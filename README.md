# emmu-pce

Core de **PC Engine / TurboGrafx-16** do Emmu, disponível tanto como biblioteca Rust nativa quanto como pacote WebAssembly. O núcleo é o `pce-core` do [JGenesis](https://github.com/jsgroth/jgenesis), vendorizado em `vendor/jgenesis` e documentado em `UPSTREAM.txt`. No desktop, PCE roda no Mesen via `emmu-mesen-sys`.

## Escopo

- Suporta **HuCard** (`.pce`).
- Não suporta jogos de CD-ROM, System Card 3.0 ou Super CD-ROM².
- Região configurada como `TurboGrafx16` para evitar jogos norte-americanos renderizando em uma única cor por bloqueio regional.
- System Card: `Arcade`.
- Aspecto NTSC, paleta `PceComposite`, resampler `WindowedSinc`, 6 canais PSG habilitados, CD-DA/ADPCM desabilitados e `crop_overscan = false`.

## API

### Nativa (`PceCore`)

- `new(rom)`, `game_title()` — HuCard não tem cabeçalho de título, então retorna `"PC Engine"`.
- `run_frame(render_graphics)` → RGBA8888 **282 × 242**.
- `take_audio_samples()` — `f32` estéreo intercalado a **48.000 Hz**.
- `audio_sample_rate()`, `audio_channels()` (`2`), `set_volume(v)`.
- `set_button(button, pressed, player)` — aceita jogadores 1 a 5. O gamepad padrão tem I/II; a UI mapeia A/B para I/II e X/Y/L/R para III–VI no modo de 6 botões.
- `restart()` — `hard_reset()` + `force_render()`, preservando RAM.
- `save_state()` / `load_state(bytes)` — magic **`EMMUPCE1`**, codificado com `bincode` sobre o estado do `PcEngineEmulator`, com SRAM em `MemorySaveWriter`.
- `save_ram()` / `load_save_ram()` — persistência separada.
- `rom()` — acesso à ROM carregada.

### WASM (`PceEmulator`)

Exporta as mesmas operações em camelCase via `wasm-bindgen`.

## Controle: por que 2 botões

`default_config()` usa `PceInputDevice::TwoButtonGamepad`.

O modo `SixButtonGamepad` alterna o estado extra a cada transição de `CLR`. Em ROMs como *Air Zonk*, *Aero Blasters* e *Bomberman '93*, isso acontecia em 214–299 de 300 frames amostrados. Quando o estado extra estava ativo, as setas não eram lidas e o menu parecia “correr sozinho” ou ignorar `Down`. Com 2 botões, o latch tradicional lê I/II + direcionais corretamente.

O teste `default_pad_is_two_button` protege essa decisão.

## Árvore de arquivos

```text
emmu-pce/
├── Cargo.toml        # cdylib + rlib; bincode, wasm-bindgen, getrandom, deps vendorizadas
├── LICENSE           # GPLv3
├── UPSTREAM.txt      # URL, commit e modificações no subset vendorizado
├── .gitignore
├── src/
│   └── lib.rs        # PceCore, PceEmulator WASM, config, save state, testes
└── vendor/
    └── jgenesis/     # subset GPL-3.0-only
        ├── Cargo.toml, Cargo.lock, LICENSE, README.md, ARCHITECTURE.md
        ├── backend/pce-core/          # VDC/VCE/PSG/CD interface
        ├── cpu/huc6280-emu/           # CPU HuC6280
        ├── common/jgenesis-common/
        ├── common/jgenesis-proc-macros/
        ├── common/dsp/
        ├── common/cdrom/              # API mínima; Emmu não carrega discos
        └── config/pce-config/
```

## Build e teste

A partir da raiz:

```sh
cargo test -p emmu-pce
```

Com ROM real:

```sh
EMMU_PCE_TEST_ROM="/caminho/para/jogo.pce" cargo test -p emmu-pce real_rom_smoke_test -- --nocapture
```

Pacote WASM:

```sh
cd crates/emmu-pce
wasm-pack build --target web \
  --out-dir ../../platforms/web/ui/src/wasm/pce \
  --out-name emmu_pce
```

O comando faz parte de `npm run build:wasm` em `platforms/web/ui`.

Testes incluídos: `default_pad_is_two_button`, `runs_frame_and_returns_rgba`, `save_state_round_trip`, `buttons_volume_and_restart` e `real_rom_smoke_test`.

## Desktop

O frontend envia `.pce` com console `PCE`. O desktop configura `SetPcEngineConfig` com `PceController`, `PceConsoleType::TurboGrafx` e a paleta padrão de `src/pce_palette.rs` do `emmu-mesen-sys`. Frames Mesen: **256 × 242**; frames WASM: **282 × 242**.

## Proveniência e licenças

| Componente | Licença | Crédito |
| --- | --- | --- |
| `emmu-pce` | GPL-3.0-only | `LICENSE` |
| JGenesis `pce-core` e subset vendorizado | GPL-3.0-only | `jsgroth/jgenesis`, commit `8d1bdfba460b3a49b493aa00368926ea375d6074` |
| `seektime.rs` dentro do vendor | MIT | Copyright (c) 2019 David Shadoff — implementação C original |
| `bincode`, `wasm-bindgen`, `getrandom` | MIT OR Apache-2.0 | dependências Rust usuais |

O arquivo `UPSTREAM.txt` registra o commit, os diretórios mantidos e as modificações locais: stub mínimo de `cdrom`, expansão de valores de workspace, remoção de `[lints] workspace = true` e `cargo fmt`. A lógica de CPU/VDC/VCE/PSG não foi alterada.
