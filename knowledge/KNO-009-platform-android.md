# KNO-009 — Alvo Android (`aarch64-linux-android` → APK)

Data e escopo: 2026-10-14, revisto em 2026-10-24. Adaptador Android de [SPEC-068](../specs/SPEC-068-multiplatform-targets.md) TGT-68-02 (`crates/gba-recomp/src/platform/android.rs`), script `scripts/ci/setup-sdl3-android.sh` e workflow `.github/workflows/android.yml`.

## Identidade e origem

- Host original: macOS arm64 (Darwin 24.3), Rust 1.99.0 com o alvo `aarch64-linux-android` (rustup), LLVM fixado 23.1.3. Revisão de 2026-10-24: Linux x86-64, mesmo Rust e LLVM, com Android SDK em `/opt/android-sdk` (NDKs r27–r30), JDK 21 e Gradle 8.14.5.
- NDK usado localmente: r28c (`Pkg.Revision = 28.2.13676358`), baixado de `dl.google.com/android/repository/android-ndk-r28c-darwin.dmg` e montado fora do repositório. Na revisão de 2026-10-24 também r30 (`30.0.15729638`), o mais alto instalado, que o adaptador escolhe sozinho. O CI de 2026-10-14 usou r30 (EV-009-10): o NDK **não** é pinado.
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

> **Correção (2026-10-24).** O parágrafo acima estava errado quando escrito: o workflow passou a rodar 11 minutos depois do commit. Ver EV-009-07 a EV-009-10 e a execução em aparelho abaixo.

## CI observado (2026-10-14) e execução em aparelho (2026-10-24)

O workflow `android.yml` executou pela primeira vez em 2026-10-14, três vezes antes da tag e uma nela:

| ID | Run | Resultado |
|---|---|---|
| EV-009-07 | `38629966634` (main push) | falha |
| EV-009-08 | `38632538650` (main push) | falha |
| EV-009-09 | `38634971812` (main push) | sucesso: SDL compilada, `assembleRelease`, `unzip -l`, `aapt`, `apksigner` |
| EV-009-10 | `38636936973` (tag `v0.1.0`) | sucesso; APK anexado à Release por `github-actions[bot]` às 21:03:45Z |

Log de EV-009-10: `Linking CXX shared library libSDL3.so`, `application-label:'Rkanoid'`, `native-code: 'arm64-v8a'`, `V2 Signer: CN=Android Debug` (SHA-256 `a837d3d6…`). O APK publicado tem 15.826.380 bytes e foi linkado com **NDK r30 (16248370)** — o adaptador escolhe o NDK mais alto instalado, então o "r28c" da seção de identidade descreve só o host da época, não um pino.

Executado localmente num Linux x86-64 com o APK recompilado a partir deste commit (evidência de execução):

| ID | Comando | Resultado |
|---|---|---|
| EV-009-11 | `adb install -r dist/rkanoid-v3.00-android-arm64.apk` em Xiaomi M2102K1AC (Android 14, SDK 34, arm64-v8a) | `Success`; atividade em primeiro plano, sem crash |
| EV-009-12 | `zipalign -c -P 16 -v 4 game.apk` | `Verification successful` (o CI não roda esta checagem) |
| EV-009-13 | `python3` procurando os bytes da ROM em `libmain.so` | ROM inteira (183.248 bytes) contígua no offset 377.952 |
| EV-009-14 | `adb logcat -s gba-recomp` após abrir o jogo | `I gba-recomp: gba-desktop: …/settings.toml is invalid (line 3: unknown key \`language\` in [])` |

EV-009-14 é a primeira prova de que os diagnósticos do runtime chegam ao logcat: antes desta mudança o `stderr` do pacote não aparecia em lugar nenhum.

O aparelho citado acima rodou o pacote Android de uma árvore com trabalho de i18n; o `settings.toml` gravado por aquele build tem uma chave `language` que a árvore de `main` ainda não conhece. O aviso é o comportamento correto ("using defaults, the file was kept"), não uma falha deste alvo.

Emulador: **não é caminho** neste host. `emulator` 37.2.12 recusa uma imagem arm64 num host x86_64 (`Avd's CPU Architecture 'arm64' is not supported by the QEMU2 emulator on x86_64 host`). Testar em aparelho real ou num host arm64.

## Limitações

- Somente `arm64-v8a`; sem armeabi-v7a, x86_64 ou App Bundle (`.aab`).
- Assinatura com a chave de debug (APK instalável para testes, não publicável em loja); sem empacotamento para loja.
- Controles touch: `crates/gba-desktop/src/touch.rs` e `draw_touch` em `app.rs` desenham o gamepad virtual (TOUCH-68-01). A pressão longa que abre o menu e o botão voltar do Android vêm do trabalho de i18n e **não** estão nesta árvore; aqui o menu continua só por teclado/botão direito. Sem party nem app de configurações (TGT-68-03).
- `bundle` para alvo cruzado **não** exige mais o runtime do host: a checagem de modo usa `--target`/`--runtime-lib` (corrigido em `cabe9f7`, registrado em [KNO-008](KNO-008-platform-ios.md)). O workflow compila `gba-native-rt` para o host apenas porque também roda o bundle nativo.
- `default_runtime` procurava o runtime cruzado dois níveis acima do binário e nunca o encontrava; `bundle --target …` sem `--runtime-lib` falhava com "runtime library … not found" mesmo com a biblioteca construída. Corrigido em `e69647c`, com teste.

## Questões abertas

- Desempenho medido em aparelho (o pacote roda, mas não foi cronometrado) e comportamento em Android 15 com páginas de 16 KiB. A instalação e a execução em arm64 já foram observadas; restam as versões.
- Ciclo de vida (pausa/retomada) e gravação do save ao sair do app pelo sistema.

## Promoção para specs e decisões

Evidência para os critérios de aceite de SPEC-068 (Android: build, link, empacotamento **e execução em aparelho arm64 observada em 2026-10-24**). O que continua pendente não é a instalação, e sim o desempenho medido, o ciclo de vida (pausa/retomada, gravação ao sair pelo sistema) e o teste num aparelho com páginas de 16 KiB.
