//! Core PC Engine / TurboGrafx-16 do Emmu.
//!
//! O núcleo de emulação é o `pce-core` do JGenesis (GPL-3.0), vendorizado em
//! `vendor/jgenesis`. Ele é Rust puro e cobre HuC6280, VDC/VCE e o PSG de 6
//! canais — ao contrário de outros cores disponíveis, não depende de
//! Emscripten para compilar para WebAssembly.
//!
//! A camada deste crate expõe a mesma API dos demais cores consumida pelo
//! worker de WebAssembly:
//!
//! - `new(rom)` / `gameTitle()`
//! - `runFrame(renderGraphics)` → RGBA8888 (282x242, incluindo overscan)
//! - `audioSamples()` / `audioSampleRate()` / `audioChannels()`
//! - `setButton(button, pressed, player)`
//! - `saveState()` / `loadState(bytes)` / `restart()`
//!
//! No desktop o PC Engine roda no Mesen via `emmu-mesen-sys`; este crate é usado
//! no navegador e também pode ser usado como biblioteca Rust nativa.

use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, ConstantInputPoller, EmulatorTrait, FrameSize, MappableInputs, RenderFrameOptions,
    Renderer, SaveWriter, TickEffect,
};
use jgenesis_common::input::Player;
use pce_config::{
    PceAspectRatio, PceButton, PceGamepadType, PceInputDevice, PceInputs, PcePaletteType,
    PcePsgResampler, PceRegion, PceSystemCardModel,
};
use pce_core::api::{PcEngineEmulator, PceEmulatorConfig};
use std::num::NonZeroU64;

/// Resolução nativa do PC Engine (pixel a pixel, antes do crop de overscan).
pub const FRAME_WIDTH: usize = 282;
/// Altura do framebuffer do VDC incluindo linhas de overscan.
pub const FRAME_HEIGHT: usize = 242;
/// Frequência de saída do resampler de áudio do `pce-core`.
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
/// Número de canais expostos ao frontend (estéreo intercalado).
pub const AUDIO_CHANNELS: u32 = 2;

/// Prefixo dos save states do Emmu, para rejeitar dados de outros cores.
const STATE_MAGIC: &[u8; 8] = b"EMMUPCE1";

/// Limite de instruções por frame. Um frame PCE tem ~357k ciclos, então o
/// limite_real é bem menor; serve apenas para evitar loop infinito se o core
/// mudar de comportamento.
const MAX_INSTRUCTIONS_PER_FRAME: u32 = 4_000_000;

fn default_config() -> PceEmulatorConfig {
    PceEmulatorConfig {
        load_disc_into_ram: false,
        always_emulate_cd_rom: false,
        // `TurboGrafx16` liga o bit de região norte-americana em MEM 0x1960.
        // Com `PcEngine` os jogos de USA (ex.: Aero Blasters) renderizam apenas
        // uma cor porque o código de boot detecta a região errada.
        region: PceRegion::TurboGrafx16,
        system_card_model: PceSystemCardModel::Arcade,
        cpu_fast_clock_divider: NonZeroU64::new(3).expect("divisor não pode ser zero"),
        aspect_ratio: PceAspectRatio::Ntsc,
        palette: PcePaletteType::PceComposite,
        crop_overscan: false,
        remove_sprite_limits: false,
        quantize_adpcm_output: false,
        psg_audio_resampler: PcePsgResampler::WindowedSinc,
        psg_channels_enabled: [true; 6],
        psg_enabled: true,
        cd_da_enabled: false,
        adpcm_enabled: false,
        psg_volume_adjustment_db: 0.0,
        cd_da_volume_adjustment_db: 0.0,
        adpcm_volume_adjustment_db: 0.0,
        // Gamepad de 6 botões mantém compatibilidade com I/II e expõe
        // III/IV/V/VI para os botões X/Y/L/R do frontend.
        input_device: PceInputDevice::SixButtonGamepad,
        turbo_tap_connected: [false; pce_config::TURBO_TAP_GAMEPADS as usize],
        turbo_tap_gamepad_types: [PceGamepadType::TwoButton;
            pce_config::TURBO_TAP_GAMEPADS as usize],
        allow_opposing_joypad_directions: false,
        allow_simultaneous_run_select: false,
    }
}

/// SRAM de bateria (HuCard com SRAM, como Populous). O `pce-core` grava via
/// `SaveWriter`; esta implementação mantém tudo em memória e expõe os bytes
/// para o frontend persistir junto com o save state.
#[derive(Debug, Default)]
struct MemorySaveWriter {
    sav: Option<Vec<u8>>,
}

impl SaveWriter for MemorySaveWriter {
    type Err = String;

    fn load_bytes(&mut self, extension: &str) -> Result<Vec<u8>, Self::Err> {
        if extension == "sav" {
            Ok(self.sav.clone().unwrap_or_default())
        } else {
            Ok(Vec::new())
        }
    }

    fn persist_bytes(&mut self, extension: &str, bytes: &[u8]) -> Result<(), Self::Err> {
        if extension == "sav" {
            self.sav = Some(bytes.to_vec());
        }
        Ok(())
    }

    fn load_serialized<D: Decode<()>>(&mut self, _extension: &str) -> Result<D, Self::Err> {
        Err("SRAM serializado não é usado pelo core PCE do Emmu.".to_owned())
    }

    fn persist_serialized<E: Encode>(
        &mut self,
        _extension: &str,
        _data: E,
    ) -> Result<(), Self::Err> {
        Ok(())
    }
}

/// Coletor de áudio. O volume é aplicado aqui para que `setVolume` funcione
/// também no navegador, que consome `audioSamples()` diretamente.
#[derive(Debug)]
struct AudioSink {
    samples: Vec<f32>,
    volume: f32,
}

impl Default for AudioSink {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            volume: 1.0,
        }
    }
}

impl AudioOutput for AudioSink {
    type Err = String;

    fn push_sample(&mut self, sample_l: f64, sample_r: f64) -> Result<(), Self::Err> {
        self.samples
            .push((sample_l * f64::from(self.volume)) as f32);
        self.samples
            .push((sample_r * f64::from(self.volume)) as f32);
        Ok(())
    }
}

/// Renderer em memória: converte o framebuffer RGBA do JGenesis em bytes RGBA8888.
#[derive(Debug)]
struct FrameRenderer {
    frame: Vec<u8>,
    width: usize,
    height: usize,
}

impl Default for FrameRenderer {
    fn default() -> Self {
        Self {
            frame: Vec::new(),
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
        }
    }
}

impl Renderer for FrameRenderer {
    type Err = String;

    fn render_frame(
        &mut self,
        frame_buffer: &[jgenesis_common::frontend::Color],
        frame_size: FrameSize,
        _target_fps: f64,
        _options: RenderFrameOptions,
    ) -> Result<(), Self::Err> {
        self.width = frame_size.width as usize;
        self.height = frame_size.height as usize;
        let pixel_count = self.width * self.height;
        self.frame.clear();
        self.frame.reserve(pixel_count * 4);
        for color in frame_buffer.iter().take(pixel_count) {
            self.frame
                .extend_from_slice(&[color.r, color.g, color.b, color.a]);
        }
        Ok(())
    }
}

/// Core Rust puro. Usado diretamente nos testes e embrulhado pelo
/// `PceEmulator` quando compilado para `wasm32`.
pub struct PceCore {
    emulator: PcEngineEmulator,
    renderer: FrameRenderer,
    audio: AudioSink,
    writer: MemorySaveWriter,
    inputs: PceInputs,
    rom: Box<[u8]>,
}

impl PceCore {
    pub fn new(rom_data: &[u8]) -> Result<Self, String> {
        // HuCards começam em $FF00 (vetores de reset/IRQ). ROMs muito curtas
        // são rejeitadas para evitar panics no barramento do JGenesis.
        if rom_data.len() < 256 {
            return Err("ROM PC Engine inválida: arquivo muito curto.".to_owned());
        }

        let config = default_config();
        let rom = rom_data.to_vec().into_boxed_slice();
        let mut writer = MemorySaveWriter::default();
        let emulator = PcEngineEmulator::create(rom.to_vec(), None, config, &mut writer);

        Ok(Self {
            emulator,
            renderer: FrameRenderer::default(),
            audio: AudioSink::default(),
            writer,
            inputs: PceInputs::default(),
            rom,
        })
    }

    pub fn game_title(&self) -> &str {
        // HuCard não tem cabeçalho de título; o frontend exibe o nome do arquivo.
        "PC Engine"
    }

    /// Avança exatamente um frame. Com `render_graphics == false` ainda
    /// avança a emulação e o áudio, mas não devolve o framebuffer.
    pub fn run_frame(&mut self, render_graphics: bool) -> Result<Vec<u8>, String> {
        self.audio.samples.clear();

        // `PceInputs` é `Copy`; o poller apenas devolve a referência.
        let inputs = self.inputs;
        let mut poller = ConstantInputPoller(&inputs);
        let mut instructions = 0_u32;

        loop {
            let effect = self
                .emulator
                .tick(
                    &mut self.renderer,
                    &mut self.audio,
                    &mut poller,
                    &mut self.writer,
                )
                .map_err(|error| error.to_string())?;

            if effect == TickEffect::FrameRendered {
                break;
            }

            instructions += 1;
            if instructions >= MAX_INSTRUCTIONS_PER_FRAME {
                return Err(
                    "O core PCE não completou um frame dentro do limite de instruções.".to_owned(),
                );
            }
        }

        if render_graphics {
            Ok(self.renderer.frame.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// Amostras estereofônicas intercaladas (L, R, L, R, ...) desde o último frame.
    pub fn take_audio_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.audio.samples)
    }

    pub fn audio_sample_rate(&self) -> u32 {
        AUDIO_SAMPLE_RATE
    }

    pub fn audio_channels(&self) -> u32 {
        AUDIO_CHANNELS
    }

    pub fn set_volume(&mut self, volume: f64) {
        self.audio.volume = volume.clamp(0.0, 1.0) as f32;
    }

    /// Atualiza um botão. O GUI envia os botões do controle clássico;
    /// I/II são os botões principais do PCE e III-VI mapeiam X/Y/L/R.
    pub fn set_button(&mut self, button: &str, pressed: bool, player: u8) -> Result<(), String> {
        let button = match button {
            "Up" => PceButton::Up,
            "Down" => PceButton::Down,
            "Left" => PceButton::Left,
            "Right" => PceButton::Right,
            "A" => PceButton::Button1,
            "B" => PceButton::Button2,
            "X" => PceButton::Button3,
            "Y" => PceButton::Button4,
            "L" => PceButton::Button5,
            "R" => PceButton::Button6,
            "Start" => PceButton::Run,
            "Select" => PceButton::Select,
            _ => return Err(format!("Botão de controle PCE desconhecido: {button}")),
        };

        let player = match player {
            1 => Player::One,
            2 => Player::Two,
            3 => Player::Three,
            4 => Player::Four,
            5 => Player::Five,
            _ => return Ok(()),
        };

        self.inputs.set_field(button, player, pressed);
        Ok(())
    }

    /// Reinicia com a ROM original, preservando o SRAM de bateria.
    pub fn restart(&mut self) {
        EmulatorTrait::hard_reset(&mut self.emulator, &mut self.writer);
        self.audio.samples.clear();
        // Reconstroi o frame atual para o frontend não mostrar a última imagem
        // do jogo anterior após um restart.
        let _ = self.emulator.force_render(&mut self.renderer);
    }

    /// Save state versionado: `EMMUPCE1` + estado bincode do `pce-core`.
    pub fn save_state(&mut self) -> Result<Vec<u8>, String> {
        let state = self.emulator.to_save_state();
        let encoded = bincode::encode_to_vec(state, bincode::config::standard())
            .map_err(|error| format!("Não foi possível salvar o estado do PCE: {error}"))?;
        let mut bytes = Vec::with_capacity(STATE_MAGIC.len() + encoded.len());
        bytes.extend_from_slice(STATE_MAGIC);
        bytes.extend_from_slice(&encoded);
        Ok(bytes)
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), String> {
        if !bytes.starts_with(STATE_MAGIC) {
            return Err("Save state PCE inválido ou de outro console.".to_owned());
        }
        let (state, _read): (PcEngineEmulator, usize) =
            bincode::decode_from_slice(&bytes[STATE_MAGIC.len()..], bincode::config::standard())
                .map_err(|error| format!("Não foi possível carregar o estado do PCE: {error}"))?;
        EmulatorTrait::load_state(&mut self.emulator, state);
        self.audio.samples.clear();
        let _ = self.emulator.force_render(&mut self.renderer);
        Ok(())
    }

    /// SRAM de bateria bruto, para persistência fora do save state.
    pub fn save_ram(&self) -> Option<Vec<u8>> {
        self.writer.sav.clone()
    }

    pub fn load_save_ram(&mut self, bytes: &[u8]) {
        self.writer.sav = Some(bytes.to_vec());
    }

    /// ROM original carregada (usada internamente pelo restart do core).
    pub fn rom(&self) -> &[u8] {
        &self.rom
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct PceEmulator {
        core: PceCore,
    }

    #[wasm_bindgen]
    impl PceEmulator {
        #[wasm_bindgen(constructor)]
        pub fn new(rom: &[u8]) -> Result<PceEmulator, JsValue> {
            Ok(Self {
                core: PceCore::new(rom).map_err(|error| JsValue::from_str(&error))?,
            })
        }

        #[wasm_bindgen(js_name = gameTitle)]
        pub fn game_title(&self) -> String {
            self.core.game_title().to_owned()
        }

        #[wasm_bindgen(js_name = runFrame)]
        pub fn run_frame(&mut self, render_graphics: bool) -> Result<Vec<u8>, JsValue> {
            self.core
                .run_frame(render_graphics)
                .map_err(|error| JsValue::from_str(&error))
        }

        #[wasm_bindgen(js_name = audioSamples)]
        pub fn audio_samples(&mut self) -> Vec<f32> {
            self.core.take_audio_samples()
        }

        #[wasm_bindgen(js_name = audioSampleRate)]
        pub fn audio_sample_rate(&self) -> u32 {
            self.core.audio_sample_rate()
        }

        #[wasm_bindgen(js_name = audioChannels)]
        pub fn audio_channels(&self) -> u32 {
            self.core.audio_channels()
        }

        #[wasm_bindgen(js_name = setButton)]
        pub fn set_button(
            &mut self,
            button: &str,
            pressed: bool,
            player: u32,
        ) -> Result<(), JsValue> {
            self.core
                .set_button(button, pressed, player as u8)
                .map_err(|error| JsValue::from_str(&error))
        }

        #[wasm_bindgen(js_name = setVolume)]
        pub fn set_volume(&mut self, volume: f64) {
            self.core.set_volume(volume);
        }

        pub fn restart(&mut self) {
            self.core.restart();
        }

        #[wasm_bindgen(js_name = saveState)]
        pub fn save_state(&mut self) -> Result<Vec<u8>, JsValue> {
            self.core
                .save_state()
                .map_err(|error| JsValue::from_str(&error))
        }

        #[wasm_bindgen(js_name = loadState)]
        pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
            self.core
                .load_state(bytes)
                .map_err(|error| JsValue::from_str(&error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_rom() -> Vec<u8> {
        let mut rom = vec![0xFF_u8; 256 * 1024];
        // Vetor de reset aponta para $FF00 ( dentro do ROM ).
        rom[0xFFFC] = 0x00;
        rom[0xFFFD] = 0xFF;
        rom[0xFFFE] = 0x00;
        rom[0xFFFF] = 0xFF;
        rom
    }

    #[test]
    fn runs_frame_and_returns_rgba() {
        let mut core = PceCore::new(&minimal_rom()).expect("ROM mínima deve carregar");
        let frame = core.run_frame(true).expect("deve avançar um frame");
        assert_eq!(frame.len(), FRAME_WIDTH * FRAME_HEIGHT * 4);
        assert_eq!(frame[3], 0xFF);
    }

    #[test]
    fn save_state_round_trip() {
        let mut core = PceCore::new(&minimal_rom()).unwrap();
        core.run_frame(true).unwrap();
        core.set_button("A", true, 1).unwrap();
        core.run_frame(true).unwrap();

        let state = core.save_state().unwrap();
        assert!(state.starts_with(STATE_MAGIC));

        core.set_button("A", false, 1).unwrap();
        core.load_state(&state).unwrap();
        assert!(core.load_state(b"ERRADO").is_err());
    }

    #[test]
    fn buttons_volume_and_restart() {
        let mut core = PceCore::new(&minimal_rom()).unwrap();
        core.run_frame(true).unwrap();
        for button in [
            "Up", "Down", "Left", "Right", "A", "B", "X", "Y", "L", "R", "Start", "Select",
        ] {
            core.set_button(button, true, 1).unwrap();
            core.set_button(button, false, 1).unwrap();
        }
        assert!(core.set_button("Desconhecido", true, 1).is_err());
        core.set_volume(0.5);
        core.restart();
        assert_eq!(core.audio_sample_rate(), 48_000);
        assert_eq!(core.audio_channels(), 2);
        assert_eq!(core.rom().len(), 256 * 1024);
    }

    #[test]
    fn real_rom_smoke_test() {
        let path = match std::env::var("EMMU_PCE_TEST_ROM") {
            Ok(path) if !path.is_empty() => path,
            _ => return,
        };
        let rom = std::fs::read(&path).expect("ROM de teste não encontrada");
        let mut core = PceCore::new(&rom).expect("ROM real deve carregar");
        let mut last_frame = Vec::new();
        let mut audio_samples = 0;
        for _ in 0..300 {
            last_frame = core.run_frame(true).expect("frame deve ser gerado");
            audio_samples += core.take_audio_samples().len();
        }
        assert_eq!(last_frame.len(), FRAME_WIDTH * FRAME_HEIGHT * 4);
        let unique = last_frame
            .chunks_exact(4)
            .collect::<std::collections::HashSet<_>>();
        eprintln!(
            "PCE teste: áudio={audio_samples}, cores únicas={}, primeiros pixels={:?}",
            unique.len(),
            &last_frame[..32]
        );
        assert!(unique.len() > 1, "frame de {path} tem uma única cor");
        assert!(audio_samples > 0, "core PCE não gerou áudio para {path}");
    }
}
