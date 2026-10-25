# Como gerar um APK (Android, `aarch64-linux-android`)

Roteiro para construir o pacote Android de um jogo de `games/` numa máquina de
desenvolvimento. O contrato está em [SPEC-068](../specs/SPEC-068-multiplatform-targets.md)
(TGT-68-02) e a evidência em [KNO-009](../knowledge/KNO-009-platform-android.md).
Aqui ficam só os passos e as armadilhas que custaram tempo.

O CI faz o mesmo em `.github/workflows/android.yml`; se o seu APK se comporta
diferente do artefato da Release, compare com aquele arquivo primeiro.

## O que precisa estar instalado

| Peça | Para quê | Como o `bundle` encontra |
|---|---|---|
| Rust + alvo `aarch64-linux-android` | compilar o runtime | `rustup target add aarch64-linux-android` |
| Android SDK | `local.properties`, `aapt`, `apksigner` | `ANDROID_HOME` (ou `ANDROID_SDK_ROOT`) |
| Android NDK | linkar `libmain.so` | `ANDROID_NDK_HOME`, `ANDROID_NDK_LATEST_HOME`, `ANDROID_NDK_ROOT`, ou o maior `$ANDROID_HOME/ndk/<versão>` |
| Gradle 8.9+ | `assembleRelease` | `GBA_GRADLE` ou `gradle` no `PATH` |
| JDK 17+ | Gradle/AGP | `JAVA_HOME` ou `PATH` |
| CMake + ninja | compilar a SDL3 | — |
| SDL3 para Android | `libSDL3.so` arm64-v8a + fontes Java | `--sdl-dir` e `SDL3_ANDROID_SOURCE` |

O NDK **não** é pinado: o adaptador usa o mais alto que encontrar. O CI de
`v0.1.0` linkou com r30 e a árvore foi validada localmente com r28c e com r30.

## 1. Compilar a SDL3 para Android

Fora do repositório, uma vez por versão do NDK:

```bash
export ANDROID_NDK_HOME=/opt/android-sdk/ndk/28.2.13676358
scripts/ci/setup-sdl3-android.sh ~/.cache/gba-recomp/sdl3-android
```

Saída: `~/.cache/gba-recomp/sdl3-android/SDL/build-android/libSDL3.so` (é o
`--sdl-dir`) e as fontes Java da SDL em `.../SDL/android-project`, que o
adaptador copia para o projeto Gradle.

## 2. Compilar o runtime e a ferramenta

```bash
export LLVM_SYS_231_PREFIX=/path/to/LLVM-23.1.3
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=\
  $ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang

cargo build --release -p gba-recomp -p gba-native-rt
cargo build --release -p gba-native-rt --target aarch64-linux-android
```

O segundo comando é o que produz `target/aarch64-linux-android/release/libgba_native_rt.a`.
Sem ele o `bundle` falha em `aot-compatible` — nunca cai para `strict-aot`.

## 3. Empacotar

```bash
export ANDROID_HOME=/opt/android-sdk
export SDL3_ANDROID_SOURCE=~/.cache/gba-recomp/sdl3-android/SDL
export GBA_GRADLE=$HOME/.gradle/wrapper/dists/gradle-8.14.5-bin/*/gradle-8.14.5/bin/gradle
export JAVA_HOME=/usr/lib/jvm/java-21-openjdk

GBA_APP_NAME="Rkanoid" target/release/gba-recomp bundle \
  --rom games/rkanoid-v3.00.gba \
  --out out/rkanoid-android \
  --target aarch64-linux-android \
  --sdl-dir "$HOME/.cache/gba-recomp/sdl3-android/SDL/build-android"
```

Saída: `out/rkanoid-android/game.apk` e `manifest.json`.

Pontos que confundem:

- **`bundle`, não `compile`.** Um APK não tem linha de comando, então o alvo
  exige a ROM embutida; `compile` é recusado antes de qualquer trabalho.
- **`--runtime-lib` é opcional.** O padrão é `target/<triple>/release/`. Se
  você moveu o alvo para outro lugar (`CARGO_BUILD_TARGET_DIR`), passe
  `--runtime-lib` ou `GBA_NATIVE_RT_LIB`.
- **`GBA_APP_NAME`** só é necessário quando o cabeçalho da ROM não traz título
  (homebrew). Sem ele o rótulo sai vazio no launcher.
- **O perfil importa.** Sem `--profile-input`, o jogo só é perfilado no menu
  inicial e mais código cai no interpretador. O CI usa
  `--profile-input 200:5:START --profile-input 300:5:A --profile-input 400:60:LEFT --profile-input 500:60:RIGHT`;
  repita isso para comparar com o artefato publicado.

## 4. Instalar e diagnosticar

```bash
adb install -r out/rkanoid-android/game.apk
adb shell am start -n dev.gbarecomp.game.g<sha12>/dev.gbarecomp.game.GameActivity
adb logcat -s gba-recomp
```

O `applicationId` é `dev.gbarecomp.game.g` + os 12 primeiros dígitos do SHA-256
da ROM; `aapt dump badging` mostra o valor exato.

Desde que os diagnósticos passaram a ir para o logcat, os avisos do frontend
aparecem com a tag `gba-recomp` — a mesma que a SDL usa. Sem isso, uma falha no
aparelho não deixava rastro nenhum. Um exemplo real:

```text
I gba-recomp: gba-desktop: /data/data/dev.gbarecomp.game.g…/files/settings.toml
  is invalid (line 3: unknown key `language` in []); using defaults, the file was kept
```

No aparelho o menu na janela abre por **pressão longa** (500 ms) numa área
livre da tela ou pelo **botão voltar**. Segurar o direcional ou um botão do
gamepad virtual não abre o menu, e arrastar também não. Se o botão voltar
fechar o app em vez de abrir o menu, o pacote foi linkado sem
`SDL_ANDROID_TRAP_BACK_BUTTON` — é um `hint` que precisa estar ativo antes do
`SDL_Init`.

Com o menu aberto, o próprio gamepad virtual vira o controle: direcional para
cima/baixo/esquerda/direita, `A` confirma, `B` volta, `Start` salta para a
última linha e `Select` para a primeira. O gamepad fica visível atrás do menu,
só como contorno, e o controle tocado acende. A navegação é **um passo por
toque**: segurar o direcional não percorre a lista, o que evita atravessar
"Reiniciar jogo" ou "Sair" sem querer.

Isso vale enquanto `input.touch_controls` não for `off`: desligar o gamepad
tira o alvo e com ele a navegação por toque do menu. Com `off` ele continua
abrindo por pressão longa e pelo botão voltar, e o botão voltar continua
voltando, mas setas e `A` ficam sem alvo — sobram o botão voltar e
dispositivos de entrada físicos.

O arquivo de configurações e o save ficam em `SDL_GetPrefPath`
(`/data/data/<package>/files/`), que não é legível por `adb shell` num APK de
release — para inspecionar, use um build `debuggable` ou leia o aviso no logcat.

## 5. Conferir o pacote

```bash
unzip -l out/rkanoid-android/game.apk | grep -E 'lib/arm64-v8a|classes.dex|AndroidManifest'
$ANDROID_HOME/build-tools/<versão>/aapt dump badging out/rkanoid-android/game.apk
$ANDROID_HOME/build-tools/<versão>/apksigner verify --print-certs out/rkanoid-android/game.apk
$ANDROID_HOME/build-tools/<versão>/zipalign -c -P 16 -v 4 out/rkanoid-android/game.apk
```

As três primeiras são o que o CI roda. A `zipalign` **não** é: ela confirma o
alinhamento de 16 KiB exigido por apps novos no Android 15+, e o `libmain.so`
já é linkado com `-Wl,-z,max-page-size=16384`.

## Emulador

Não é caminho num host x86_64: o emulador recusa uma imagem arm64
(`Avd's CPU Architecture 'arm64' is not supported by the QEMU2 emulator on
x86_64 host`). Use aparelho real, ou um host arm64 com imagem arm64.

## O que ainda não está coberto

Instalação e execução em aparelho arm64 já foram observadas. Continuam abertos:
desempenho medido, ciclo de vida (pausa/retomada e gravação ao sair pelo
sistema) e Android 15 com páginas de 16 KiB. Ver as questões abertas em
[KNO-009](../knowledge/KNO-009-platform-android.md).
