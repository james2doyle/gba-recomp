# KNO-008 — Alvo iOS (SPEC-068 TGT-68-02)

Data e escopo: 2026-10-14. Alvos `aarch64-apple-ios` (dispositivo) e `aarch64-apple-ios-sim` (simulador em Apple silicon). Host: Darwin 24.3.0 arm64, Xcode com iPhoneOS/iPhoneSimulator SDK 18.5, Rust 1.99.0, LLVM 23.1.3 (`LLVM-23.1.3-macOS-ARM64`), SDL 3.4.12 (`release-3.4.12`) compilada do fonte.

## Identidade e origem

- Runtime: `cargo build --release -p gba-native-rt --target aarch64-apple-ios[-sim]` → `libgba_native_rt.a` (staticlib).
- SDL3: `scripts/ci/setup-sdl3-ios.sh <dest>` clona `release-3.4.12` e gera `libSDL3.a` estática com CMake (`CMAKE_SYSTEM_NAME=iOS`, `CMAKE_OSX_SYSROOT=iphoneos|iphonesimulator`, arm64, deployment target 14.0, `SDL_SHARED=OFF`). Saída: `<dest>/iphoneos/lib` e `<dest>/iphonesimulator/lib`; no CI exporta `SDL3_IOS_DIR`/`SDL3_IOS_SIM_DIR`, que `gba-recomp` usa como `--sdl-dir` padrão.
- Adaptador: `crates/gba-recomp/src/platform/ios.rs`. Workflow: `.github/workflows/ios.yml`.

## Desenho

- Ponto de entrada: o objeto gerado define `SDL_main` (TargetSpec::entry_symbol). Sob `cfg(target_os = "ios")`, `gba-native-rt` define o `main` do processo, que chama `SDL_RunApp(argc, argv, SDL_main, NULL)`; a SDL inicia o UIKit e chama `SDL_main` → `gba_native_main`. O frontend padrão já é a janela (sem `--headless`).
- Link: clang do LLVM fixado, `-target arm64-apple-ios14.0[-simulator]`, `-fuse-ld=lld`, `-isysroot` do SDK, objetos + runtime + `libSDL3.a` + frameworks listados no `sdl3.pc` da SDL (CoreMedia, CoreVideo, CoreAudio, AudioToolbox, AVFoundation, CoreBluetooth, CoreGraphics, CoreMotion, Foundation, GameController, Metal, OpenGLES, QuartzCore, UIKit, CoreHaptics fraco) + `-lobjc -lc++`.
- Pacote: `.app` plano (`game`, `Info.plist`, `PkgInfo`, `manifest.json`, ícones `AppIcon60x60@2x/@3x`, `AppIcon76x76@2x~ipad`, `AppIcon83.5x83.5@2x~ipad` gerados com `sips` de `assets/logo-ico.png`, via `CFBundleIconFiles`, sem asset catalog). `Info.plist`: identificador `dev.gba-recomp.game.<sha12>`, `CFBundleSupportedPlatforms` `iPhoneOS`/`iPhoneSimulator`, `MinimumOSVersion` 14.0, `UILaunchScreen`, `UIRequiresFullScreen`, orientações paisagem + retrato, `UIDeviceFamily` iPhone e iPad.
- Simulador: `.app` assinado ad hoc (`codesign -s -`). Dispositivo: `.app` sem assinatura de bundle e `<saída>.ipa` (zip de `Payload/<nome>.app`) criado após a publicação (`platform::after_publish`).
- Somente `bundle` (ROM embutida): iOS não tem linha de comando; `compile` para iOS falha com mensagem clara. SDL ou runtime ausentes também falham com mensagem que aponta o script/`--sdl-dir`.
- Configurações e saves ficam no sandbox: `settings::default_path` usa `$HOME/Library/Application Support/gba-recomp/settings.toml` também em iOS (em iOS `HOME` é o contêiner do app); o save padrão é `$HOME/Library/Application Support/gba-recomp/saves/<sha256>.sav`.
- Correção lateral: a checagem de disponibilidade do modo `aot-compatible` procurava sempre o runtime do host; agora usa `--target`/`--runtime-lib`, permitindo `bundle` cruzado sem runtime do host.

## Achados

Observação de execução (comandos efetivamente rodados neste host):

| ID | Comando | Resultado |
|---|---|---|
| EV-008-01 | `scripts/ci/setup-sdl3-ios.sh <scratch>` | exit 0; `libSDL3.a` de 3,7 MiB para `iphoneos` e `iphonesimulator` |
| EV-008-02 | `cargo build --release -p gba-native-rt --target aarch64-apple-ios` e `--target aarch64-apple-ios-sim` | ok; `nm` mostra `T _main`, `T _gba_native_main`, `U _SDL_RunApp`, `U _SDL_main` |
| EV-008-03 | `gba-recomp bundle --rom games/rkanoid-v3.00.gba --out dist/Rkanoid-ios --target aarch64-apple-ios --runtime-lib … --sdl-dir <sdl>/iphoneos/lib` | `dist/Rkanoid-ios.app` (executável 5,5 MB) e `dist/Rkanoid-ios.ipa` (3,4 MB; `unzip -l` mostra `Payload/Rkanoid-ios.app/…`) |
| EV-008-04 | idem com `--target aarch64-apple-ios-sim` | `dist/Rkanoid-ios-sim.app`; `codesign --verify` válido (adhoc, `Sealed Resources` v2) |
| EV-008-05 | `plutil -lint` nos dois `Info.plist` | OK |
| EV-008-06 | `lipo -info` / `otool -l` (`LC_BUILD_VERSION`) | arm64 thin; dispositivo `platform 2` (IOS), simulador `platform 7` (IOSSIMULATOR); `minos 14.0`, `sdk 18.5` |
| EV-008-07 | `otool -L` do executável de dispositivo | só frameworks do sistema iOS, `libobjc`, `libc++`, `libSystem`; nenhum AppKit; SDL estática (`_SDL_RunApp` definido no executável) |
| EV-008-08 | `cargo test --release -p gba-recomp --bin gba-recomp`, `-p gba-native-rt -p gba-desktop` | ok (126 testes em gba-recomp, incluindo os de `platform::ios`) |

## Não verificado (limites)

- **Execução**: `xcrun simctl list runtimes` não lista nenhum runtime de simulador neste Mac; nenhum runtime foi baixado (vários GB). O app **não foi instalado nem executado** no simulador nem em dispositivo. Pela regra 12 do AGENTS, iOS ainda **não** é alvo suportado: há emissão, link e empacotamento verificados, falta execução e adaptadores (touch, áudio, ciclo de vida) verificados.
- **Assinatura**: o `.ipa` não é assinado. Para instalar em aparelho real é preciso re-assinar com certificado Apple e provisioning profile próprios (Xcode, `codesign` + `embedded.mobileprovision`, ou ferramentas de sideload). O executável de dispositivo pode conter a assinatura ad hoc que o `ld64.lld` aplica a binários arm64; isso não é assinatura de bundle e não permite instalação.
- **Título**: a ROM livre de `games/` tem título de cabeçalho vazio (0xA0–0xAB zerados), então `CFBundleName` cai em "GBA Game", igual ao macOS.
- Inferência (não testada): a SDL usa `UILaunchScreen` vazio e cria a janela em tela cheia; o comportamento ao ir para segundo plano (pausa, gravação do save) depende do frontend, que não trata eventos de ciclo de vida específicos de iOS. O save é persistido pelos pontos já existentes do frontend (menu/saída); se o iOS encerrar o app em segundo plano, progresso não salvo pode ser perdido.
- Controles touch (TOUCH-68-01) são de outro trabalho; sem eles e sem controle físico o jogo não é jogável no aparelho.
- O workflow `ios.yml` não foi executado neste host (só escrito); o release em tag compartilha a mesma Release com `ci.yml` via `softprops/action-gh-release@v2`.

## Questões abertas

- Tratar `SDL_EVENT_WILL_ENTER_BACKGROUND`/`TERMINATING` para gravar o save.
- Asset catalog (`Assets.car` via `actool`) para ícones em iOS recentes e marketing icon 1024.
- XCFramework do runtime para integração com projetos Xcode.

## Promoção para specs e decisões

Status de TGT-68-02 (iOS) no ROADMAP/SPEC-068 fica a cargo do coordenador: link e empacotamento verificados localmente; execução pendente.
