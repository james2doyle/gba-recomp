# SPEC-068 — Alvos multiplataforma, controles touch e CI de release

Estado do documento: Accepted em 2026-10-14; implementada. Verificação (release v0.1.0, CI verde em todos os workflows): macOS arm64 (local e CI), macOS x86-64 (cross-build no runner arm64, `--verify-interpreter` sob Rosetta 2), Linux x86-64 e ARM64 e Windows x86-64 (bundle do Rkanoid verificado contra o interpretador no CI); Web (bundle no CI, publicado no GitHub Pages e jogado em navegador Chromium, KNO-010); iOS (`.ipa` não assinado e app de simulador gerados e inspecionados, nunca executados, KNO-008); Android (APK debug gerado e validado com `aapt`/`apksigner` no CI, nunca executado em aparelho, KNO-009). Controles touch: testes do mapeamento e smoke em janela; não testados em tela touch real. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Pacotes `aot-compatible` só existem hoje para macOS arm64. Esta spec define como `gba-recomp` gera pacotes para Windows, Linux (x86-64 e ARM64, incluindo Raspberry Pi e Orange Pi), macOS, iOS, Android e Web (WebAssembly), como o frontend oferece controles em tela para telas touch e como o CI compila, empacota e publica releases do jogo livre em `games/`. Ela especializa [SPEC-010](SPEC-010-platform-packaging.md) e SPEC-062/063/066.

## Termos

- **Host de build**: máquina onde `gba-recomp` roda. **Alvo**: triple do pacote gerado.
- **Runtime do alvo**: `gba-native-rt` + `gba-desktop` + `gba-core` compilados para o alvo (biblioteca estática), mais SDL3 do alvo.

## Requisitos normativos

- TGT-68-01: `compile`/`bundle` MUST aceitar `--target <triple>` (padrão: host), `--runtime-lib <arquivo>` e `--sdl-dir <dir>`. O LLVM emite objetos para o triple pedido (backends X86, AArch64, ARM, WebAssembly); o link e o empacotamento usam o adaptador da plataforma. Alvo sem adaptador MUST falhar antes de qualquer trabalho, sem pacote parcial.
- TGT-68-02: alvos e formatos: `aarch64-apple-darwin`/`x86_64-apple-darwin` → `.app`; `x86_64-unknown-linux-gnu`/`aarch64-unknown-linux-gnu`/`armv7-unknown-linux-gnueabihf` → diretório com `game`, `libSDL3.so.0` (rpath `$ORIGIN`) e companions, mais `.tar.gz` no CI; `x86_64-pc-windows-msvc` → diretório com `game.exe`, `SDL3.dll`, companions e `.zip`; `aarch64-apple-ios` → `.app` iOS (sem assinatura no CI, `.ipa` não assinado); `aarch64-linux-android` → APK (SDL3 Android, `libmain.so`); `wasm32-unknown-emscripten` → `index.html` + `.js` + `.wasm`.
- TGT-68-03: companions (`gba-settings`, `gba-party`) MUST acompanhar os alvos desktop; em iOS, Android e Web as configurações ficam no menu dentro da janela e a party fica fora do escopo inicial (Web: sem tokio/webrtc-rs; um adaptador WebRTC do navegador é trabalho futuro).
- TGT-68-04: a equivalência NB-62-01 MUST valer em todos os alvos; `--headless --verify-interpreter` é o teste de aceite nos alvos que executam no CI (Linux x86-64/ARM64, macOS, Windows).
- TOUCH-68-01: o frontend MUST desenhar um gamepad virtual (direcional, A, B, L, R, Start, Select) semitransparente quando `input.touch_controls` for `auto` e houver dispositivo touch, ou `on`; `off` desativa. Toques multipontos via eventos de dedo da SDL; cada botão vira bit de KEYINPUT combinado com os demais dispositivos e com neutralização de opostos. Na Web a opção existe e é desligada por padrão. Os controles nunca alteram a simulação além do KEYINPUT (SET-63-02).
- CI-68-01: o CI MUST, em cada push/PR, compilar e testar o workspace em Linux, macOS e Windows; em tag `v*` MUST gerar `bundle` de cada jogo de `games/` para cada alvo suportado e anexar os artefatos a uma GitHub Release. Jogos fora de `games/` nunca entram no CI.

## Critérios de aceite

Linux ARM64 verificado localmente em contêiner; macOS verificado localmente; demais alvos verificados pelo CI. Cada alvo registra o que foi efetivamente executado (build, link, execução headless, execução em dispositivo real). Controles touch: testes do mapeamento de toques e smoke em janela.

## Referências

[SPEC-010](SPEC-010-platform-packaging.md), [SPEC-062](SPEC-062-native-blocks-aot-compatible.md), [SPEC-063](SPEC-063-recompiled-frontend-settings.md), [SPEC-066](SPEC-066-bundle-embedded-rom.md).
