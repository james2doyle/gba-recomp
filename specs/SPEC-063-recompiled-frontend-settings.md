# SPEC-063 — Frontend do pacote recompilado, configurações e menu de contexto nativo

Estado do documento: Accepted em 2026-10-14. Implementada: `gba-desktop` virou biblioteca (`run_app`) usada pelo binário interpretado e pelo runtime `gba-native-rt`; o pacote abre janela por padrão com a SDL3 embutida (`@rpath`, re-assinada ad hoc); `settings.toml` com vídeo/áudio/controles; menu na janela por teclado/gamepad com rascunho/Aplicar/Cancelar e remapeamento; menu de contexto nativo `NSMenu` no macOS (botão direito), menu da janela no Linux. Verificado em macOS arm64: testes unitários de settings/input/menu (13), smoke da janela com drivers SDL `dummy` (LeafGreen O0 300 frames e O2 600 frames, saída 0, áudio ligado) e `--headless --verify-interpreter` O2 por 3000 frames com input sem divergência. **Não verificado ainda**: janela real, `NSMenu` na tela, som real e gamepad físico (exigem teste manual); Linux não foi compilado. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

O pacote `aot-compatible` da [SPEC-062](SPEC-062-native-blocks-aot-compatible.md) só executa headless. Esta spec torna o executável recompilado jogável (janela, áudio, controles, save) reutilizando o frontend SDL3 da [SPEC-061](SPEC-061-runner-frontend.md), e implementa o subconjunto de [SPEC-009](SPEC-009-settings-menu.md) para vídeo, áudio e controles, acessível por um menu dentro da janela (teclado/gamepad) e pelo menu de contexto nativo de cada sistema (botão direito).

## Fora de escopo

Overrides por jogo e página "Jogo" da SPEC-009, seleção de dispositivo de áudio específico, turbo/macros, UI-04 com reversão automática em 15 s (fullscreen é reversível pelo mesmo atalho/menu e registrado como limitação), Windows (sem host suportado ainda).

## Requisitos normativos

- FR-63-01: `gba-desktop` MUST expor uma biblioteca `run_app(options, native)` usada pelo binário interpretado e pelo runtime `gba-native-rt`; com `native` presente cada fronteira de instrução é oferecida aos runs nativos exatamente como em `--headless`. O título da janela MUST indicar o modo (`interpreted` ou `aot-compatible`).
- FR-63-02: o executável do pacote MUST abrir janela por padrão; `--headless` mantém o CLI da SPEC-062. O pacote MUST conter a biblioteca SDL3 usada (copiada e referenciada por `@executable_path`/`$ORIGIN`), sem exigir LLVM.
- SET-63-01: configurações em `settings.toml` (`version = 1`) no diretório de configuração do usuário (macOS `~/Library/Application Support/gba-recomp/`, Linux `$XDG_CONFIG_HOME/gba-recomp/` ou `~/.config/gba-recomp/`); `--settings PATH` sobrescreve. Seções `video` (escala 1–8, tela cheia, filtro nearest/linear, escala inteira, vsync, correção de cor LCD), `audio` (volume 0–100, mudo, latência 20–200 ms), `input` (bindings de teclado e gamepad por botão GBA, deadzone 0–0,95 do analógico esquerdo como direcional). Escrita atômica; arquivo inválido é preservado, defaults seguros são usados e o diagnóstico é exibido (UI-07).
- SET-63-02: nenhuma configuração MUST alterar a simulação: hash do framebuffer e ciclos com os mesmos inputs são idênticos para qualquer vídeo/áudio (SPEC-009 matriz final). Correção de cor e filtro atuam só na apresentação.
- MENU-63-01: menu na janela abre por `Esc`, `F1`, botão direito (Linux), `Guide`/`Back+Start` no gamepad ou, em tela de toque, por pressão longa (500 ms) numa área livre da tela e pelo botão voltar do Android. A pressão longa MUST NOT disparar sobre os controles na tela (segurar um direcional é jogo, não gesto) e MUST disparar no máximo uma vez por pressão; o botão voltar MUST NOT ser capturável como binding de jogo, para que o menu continue alcançável num aparelho sem teclado. Navega por setas/direcional, confirma com `Enter`/A, volta com `Esc`/B; pausa o guest e o áudio (UI-06) e retoma sem acumular tempo host. Páginas Vídeo, Áudio, Controles e Sistema (continuar, reiniciar, salvar agora, sair). Edições ficam em rascunho até "Aplicar"; "Cancelar" restaura os valores efetivos (UI-02). Em tela touch o gamepad virtual assume a navegação enquanto o menu está aberto ([SPEC-068](SPEC-068-multiplatform-targets.md) TOUCH-68-02).
- MENU-63-02: remapeamento mostra a ação aguardada, aceita tecla ou botão de gamepad, `Esc` cancela mantendo o binding anterior (UI-01). Direções opostas simultâneas são neutralizadas (INPUT-01). Perda de foco e desconexão de gamepad liberam botões (INPUT-02).
- CTX-63-01: o botão direito MUST abrir o menu de contexto nativo do sistema quando houver adaptador: no macOS um `NSMenu` com submenus Vídeo (escala, tela cheia, filtro, escala inteira, vsync, correção de cor), Som (mudo, volume, latência), Controles (deadzone, abrir remapeamento, restaurar padrões) e Sistema (pausar, reiniciar, salvar agora, sair), com marcas no valor efetivo. Ações do menu de contexto aplicam e persistem imediatamente. Sem adaptador nativo (Linux), o botão direito abre o menu da janela.

- FR-63-03: a janela do jogo MUST exibir `assets/logo-alpha.png` como splash por 2,4 s (fade de entrada/saída, na resolução de saída) antes de iniciar o guest; `--no-splash` desativa para execuções automatizadas. `assets/logo-ico.png` MUST ser o ícone da janela/Dock do jogo e de `gba-settings`. Os dois PNGs são embutidos no binário e decodificados pela SDL (`SDL_LoadPNG_IO`).

- CTX-63-02: no macOS a barra de menus MUST oferecer, no menu do aplicativo, "Configurações…" (⌘,) abrindo `gba-settings` (reaproveitando o item de preferências criado pela SDL) e menus de topo Vídeo, Som, Controles e Sistema com as mesmas ações e marcas do menu de contexto, atualizados quando as configurações mudam. Verificado com `GBA_DEBUG_MENUBAR=1`, que lista a barra montada.

## Critérios de aceite

Testes de parse/serialização/migração de `settings.toml` (válido, inválido preservado, faixas), mapeamento de input com deadzone e neutralização de opostos, navegação/rascunho do menu em modelo puro; smoke do pacote recompilado com driver de vídeo `dummy` por N frames; hash idêntico com configurações diferentes; verificação manual em macOS de janela, menu nativo, som e gamepad registrada como evidência.

## Referências

[SPEC-009](SPEC-009-settings-menu.md), [SPEC-010](SPEC-010-platform-packaging.md), [SPEC-061](SPEC-061-runner-frontend.md), [SPEC-062](SPEC-062-native-blocks-aot-compatible.md).
