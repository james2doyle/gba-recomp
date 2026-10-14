# KNO-010 — Alvo Web (WebAssembly/Emscripten)

Data e escopo: 2026-10-14. Alvo `wasm32-unknown-emscripten` de [SPEC-068](../specs/SPEC-068-multiplatform-targets.md) TGT-68-02/TGT-68-03: `gba-recomp bundle --target wasm32-unknown-emscripten` gera um site estático (`index.html`, `game.js`, `game.wasm`, `logo.png`, `manifest.json`) com a ROM embutida.

## Identidade e origem

- Host: macOS arm64 (Darwin 24.3.0); Rust 1.99.0 (LLVM 23.1.1 do rustc); LLVM fixado 23.1.3 para a emissão.
- Emscripten 6.0.12 (`emsdk install latest`, instalado fora do repositório, em diretório temporário); porta SDL3 baixada pelo `emcc` (`release-3.4.2`).
- ROM: `games/rkanoid-v3.00.gba`, SHA-256 `99ae825ffb1b7e8b48b33cd2715ceb8ca524858f99fee4b22938489460083d94`.
- Código: `crates/gba-recomp/src/platform/web.rs` (+ `web/index.html`), `crates/gba-desktop/src/pace.rs`, `crates/gba-desktop/build.rs`, `.github/workflows/web.yml`.

## Achados

Observação (execução local):

- O `std` do Rust para `wasm32-unknown-emscripten` usa `panic="unwind"` com exceções wasm (símbolos `__cpp_exception`, `_Unwind_RaiseException` em `libgba_native_rt.a`). O link final precisa de `-fwasm-exceptions`; `-sASYNCIFY` + `-fwasm-exceptions` linkou sem erro com Emscripten 6.0.12.
- No WebAssembly o clang chama `int main(int, char**)` de `__main_argc_argv`, e o crt do Emscripten chama `main` sem argumentos. O emissor passou a nomear a entrada `__main_argc_argv` nesse alvo (`TargetSpec::entry_symbol`); `llvm-nm table.o` mostra `T __main_argc_argv` e `U gba_native_main`.
- `llvm-objdump -h runs0.o`: `file format wasm`, seções TYPE/IMPORT/FUNCTION/CODE/linking/reloc.CODE/target_features.
- O laço SDL de `gba-desktop` é bloqueante. Com `-sASYNCIFY`, `pace::sleep`/`pace::yield_now` chamam `emscripten_sleep` a cada quadro (inclusive quando atrasado ou em avanço rápido), devolvendo o controle ao navegador; em desktop continuam `thread::sleep`/no-op.
- Antes desta mudança, `compile`/`bundle` em modo `aot-compatible` exigiam o runtime do host mesmo com `--target`/`--runtime-lib`; a checagem agora usa o runtime do alvo pedido.
- No navegador (painel de browser do Claude Code, Chromium): o site servido por `python3 -m http.server` mostra o logo, "Clique ou pressione uma tecla para jogar"; após o clique o jogo roda (splash DreamSoft, tela título do Arkanoid Advance), Enter (Start) é aceito. Console: apenas o aviso do SDL sobre `ScriptProcessorNode` obsoleto; nenhum erro. Canvas interno 720×480 escalado por CSS para 927×608 (3:2).

Inferência (não medida):

- ASYNCIFY instrumenta funções que podem chegar a `emscripten_sleep`, inclusive via chamadas indiretas (runs nativos chamados por ponteiro): espera-se custo de tamanho (`game.wasm` ≈ 4,9 MB) e de desempenho. Não foi medida a taxa de quadros no navegador nem comparada a `-sJSPI`.
- Saves: `/save` é um IDBFS; a página restaura antes de `main` e sincroniza a cada 5 s, ao esconder a aba e em `pagehide`. Rkanoid não gravou save durante o teste, então a persistência entre recarregamentos não foi observada.

Limitações:

- Sem party/WebRTC na Web (sem tokio/webrtc-rs; um adaptador WebRTC do navegador é trabalho futuro) e sem companions (`gba-settings`, `gba-party`): o item "Configurações externas" do menu falha com mensagem; as opções ficam no menu dentro da janela (F1/Esc).
- Áudio só começa depois de um gesto do usuário (regra dos navegadores): por isso o jogo só inicia após clique/tecla no overlay.
- Esc também sai da tela cheia do navegador; F11 é capturado pelo navegador.
- `--verify-interpreter` (NB-62-01) não roda no navegador; a equivalência continua verificada nos alvos nativos do CI.
- O título vem do cabeçalho da ROM; Rkanoid tem título vazio e a página mostra "GBA".

## Reprodução e resultado

```sh
rustup target add wasm32-unknown-emscripten
cargo build --release -p gba-native-rt --target wasm32-unknown-emscripten   # ok, libgba_native_rt.a ~15 MB
cargo build --release -p gba-recomp                                         # host
gba-recomp bundle --rom games/rkanoid-v3.00.gba --out dist/Rkanoid-web \
  --target wasm32-unknown-emscripten \
  --runtime-lib target/wasm32-unknown-emscripten/release/libgba_native_rt.a \
  --profile-input 200:5:START --profile-input 300:5:A --profile-input 400:60:LEFT --profile-input 500:60:RIGHT
# ok: 184 runs, emit 0.66 s, emcc link 4.6 s; game.js 207 KB, game.wasm 4.9 MB
python3 -m http.server 8765 --bind 127.0.0.1 --directory dist/Rkanoid-web
```

`emcc` é procurado em `EMCC`, no `PATH` e em `$EMSDK/upstream/emscripten`; sem ele o `bundle` falha antes de perfilar, sem pacote parcial. Sem ROM embutida (`compile` sem `--embed-rom`) o alvo Web também é recusado antes do trabalho.

CI (`.github/workflows/web.yml`, ainda não executado): Ubuntu 24.04, LLVM 23.1.3, emsdk 6.0.12, build do host e do runtime wasm, `bundle` de cada `games/*.gba`, artefato zip, anexo na release em tags `v*` e deploy no GitHub Pages em `main` e tags (requer Pages configurado com "GitHub Actions" como fonte).

## Questões abertas

- Desempenho com ASYNCIFY vs JSPI; `-sASYNCIFY_IGNORE_INDIRECT` não foi avaliado.
- Persistência de saves em IDBFS entre recarregamentos com um jogo que grave backup.
- Controles touch na Web (TOUCH-68-01, desligados por padrão) dependem do trabalho paralelo em `app.rs`.
- Navegadores além de Chromium (Firefox, Safari) não testados.

## Promoção para specs e decisões

Atende parcialmente TGT-68-02 (Web): emissão, link e execução no navegador observados localmente; publicação pelo CI pendente da primeira execução.
