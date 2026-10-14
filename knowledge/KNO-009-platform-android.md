# KNO-009 — Alvo Android (`aarch64-linux-android` → APK)

Data e escopo: 2026-10-14. Adaptador Android de [SPEC-068](../specs/SPEC-068-multiplatform-targets.md) TGT-68-02 (`crates/gba-recomp/src/platform/android.rs`), script `scripts/ci/setup-sdl3-android.sh` e workflow `.github/workflows/android.yml`.

## Identidade e origem

- Host local: macOS arm64 (Darwin 24.3), Rust 1.99.0 com o alvo `aarch64-linux-android` (rustup), LLVM fixado 23.1.3.
- NDK usado localmente: r28c (`Pkg.Revision = 28.2.13676358`), baixado de `dl.google.com/android/repository/android-ndk-r28c-darwin.dmg` e montado fora do repositório. Sem Android SDK, Java ou Gradle locais.
- SDL: tag `release-3.4.12` de `github.com/libsdl-org/SDL`, compilada fora do repositório com o `android.toolchain.cmake` do NDK (`ANDROID_ABI=arm64-v8a`, `ANDROID_PLATFORM=24`).
- Ícones `src/platform/android/res/mipmap-*/ic_launcher.png`: derivados de `assets/logo-ico.png` (1254×1254) com `sips -z` em 48/72/96/144/192 px.

## Achados

Fluxo implementado (fonte):

1. `bundle --target aarch64-linux-android` emite objetos LLVM para `aarch64-linux-android24`; o símbolo de entrada gerado é `SDL_main`.
2. O adaptador exige ROM embutida (APK não tem linha de comando), localiza `libSDL3.so` em `--sdl-dir`, as fontes Java da SDL (`android-project/app/src/main/java/org/libsdl/app`, via `SDL3_ANDROID_SOURCE` ou subindo a partir de `--sdl-dir`), o NDK (`ANDROID_NDK_HOME`, `ANDROID_NDK_LATEST_HOME`, `ANDROID_NDK_ROOT`, ou `$ANDROID_HOME/ndk/<maior versão>`), o SDK (`ANDROID_HOME`) e o Gradle (`GBA_GRADLE` ou `gradle` no PATH) **antes** de linkar; qualquer ausência falha com mensagem clara e sem pacote parcial.
3. Linka `libmain.so` com `aarch64-linux-android24-clang` do NDK: `-shared -Wl,-soname,libmain.so`, `-lSDL3 -llog -landroid -lm -ldl -lc`, `--no-undefined`, `--gc-sections`, páginas de 16 KiB.
4. Gera um projeto Gradle mínimo (AGP 8.7.3, compileSdk/targetSdk 35, minSdk 24) com `dev.gbarecomp.game.GameActivity extends SDLActivity` (`getLibraries() = {"SDL3","main"}`), manifesto paisagem (`sensorLandscape`) e tela cheia, rótulo = título do cabeçalho da ROM (ou `GBA_APP_NAME`), `applicationId` `dev.gbarecomp.game.g<sha12>`.
5. `gradle assembleRelease` com `signingConfig signingConfigs.debug`; o pacote final contém `game.apk` e `manifest.json`.

Runtime no Android (fonte): configurações em `SDL_GetPrefPath` (armazenamento interno do app) em vez de HOME/XDG; save padrão `<pref>/<sha256[..16]>.sav` quando não há `--save`; `gba-settings`/`gba-party` não são lançados (TGT-68-03); frontend em janela por padrão (sem `--headless`). O GPIO e o menu de contexto nativo já eram restritos a Linux/macOS por `cfg`.

## Reprodução e resultado

Executado localmente (evidência de execução):

| ID | Comando | Resultado |
|---|---|---|
| EV-009-01 | `cargo check -p gba-native-rt --target aarch64-linux-android` | OK |
| EV-009-02 | `cargo build --release -p gba-native-rt --target aarch64-linux-android` | `libgba_native_rt.a` (~27 MB) sem precisar de linker do NDK |
| EV-009-03 | `scripts/ci/setup-sdl3-android.sh <dir>` com NDK r28c | `libSDL3.so` ELF aarch64 e `SDLActivity.java` presentes |
| EV-009-04 | `cargo test -p gba-recomp --bin gba-recomp android` | 3 testes OK (layout do projeto Gradle, escape de strings, ordem de versões do NDK) |
| EV-009-05 | `bundle --rom games/rkanoid-v3.00.gba --target aarch64-linux-android ...` sem Gradle no PATH | Falha: "Gradle not found ..."; diretório de saída vazio |
| EV-009-06 | Mesmo `bundle` com `GBA_GRADLE` apontando para um stub que copia o projeto e sai com erro | Link do `libmain.so` OK (`--no-undefined`); `NEEDED` = libSDL3/liblog/libandroid/libm/libdl/libc; `SONAME` libmain.so; `SDL_main` e `gba_native_main` exportados; `LOAD` alinhado em 0x4000; projeto Gradle com Java da SDL, jniLibs, ícones, rótulo `Rkanoid`; saída vazia após a falha |

Somente no CI (`android.yml`, ubuntu-24.04, ainda não observado): build do SDL com o NDK do runner, `gradle assembleRelease` (Java 17, Gradle 8.12, AGP 8.7.3), `unzip -l`, `aapt dump badging` e `apksigner verify` do APK, upload do artefato e anexo à Release em tags `v*`.

Inferência, não verificada: o APK instala e roda em um dispositivo/emulador arm64. Nenhuma execução em Android real ou emulador foi feita; `--verify-interpreter` não roda no CI para Android (TGT-68-04 só exige nos alvos executáveis no CI).

## Limitações

- Somente `arm64-v8a`; sem armeabi-v7a, x86_64 ou App Bundle (`.aab`).
- Assinatura com a chave de debug (APK instalável para testes, não publicável em loja); sem empacotamento para loja.
- Sem controles touch próprios aqui (trabalho de TOUCH-68-01 em `app.rs`, outro agente); sem party nem app de configurações.
- `stderr` do runtime não vai para o logcat.
- `bundle` para alvo cruzado ainda exige a biblioteca de runtime do host (verificação `require_available_mode` em `main.rs`); o workflow compila `gba-native-rt` também para o host.

## Questões abertas

- Execução real em dispositivo/emulador (Android 7+ e Android 15 com páginas de 16 KiB) e medição de desempenho.
- Ciclo de vida (pausa/retomada) e gravação do save ao sair do app pelo sistema.

## Promoção para specs e decisões

Evidência para os critérios de aceite de SPEC-068 (Android: build, link e empacotamento; execução pendente).
