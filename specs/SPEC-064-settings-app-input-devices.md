# SPEC-064 — Janela externa de configurações e dispositivos de entrada (USB, Bluetooth, GPIO)

Estado do documento: Accepted em 2026-10-14. Implementada: binário `gba-settings` (egui/eframe 0.36.2 com `glow`), aberto por "Configurações…" no menu de contexto e por "Janela de configuracoes..." no menu SDL, incluído no pacote; o jogo recarrega `settings.toml` em até 0,5 s; joysticks genéricos (`joy_*`) e GPIO pela uAPI v2 em `gba-desktop`. Verificado em macOS arm64: testes de modelo/parser/nomes de tecla, `cargo check` para `aarch64-unknown-linux-gnu` (o módulo GPIO compila com as asserções de layout de 592/272/16 bytes), app aberto por 5–6 s sem erro (direto e de dentro do pacote) e smoke do jogo com driver dummy. No macOS o app desliga o HIDAPI da SDL (o enumerador roda o run loop do Cocoa dentro do winit e abortava); a detecção segue pelo IOKit/GameController. **Pendente**: uso interativo da janela, captura com gamepad/joystick físicos e teste em Raspberry Pi com pinos reais. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

O menu desenhado dentro da janela SDL ([SPEC-063](SPEC-063-recompiled-frontend-settings.md)) é limitado (fonte 8×8 sem acentos, sem mouse) e não cobre joysticks genéricos nem GPIO. Esta spec acrescenta uma janela de configurações externa, separada do jogo, sem remover o menu SDL, e amplia a entrada para gamepads USB/Bluetooth, joysticks genéricos e botões ligados a GPIO (Raspberry Pi e similares).

## Decisões

- A janela é o binário `gba-settings`, escrito com egui/eframe (renderer `glow`), um único código para macOS, Linux (inclusive Raspberry Pi) e, futuramente, Windows. As dependências são do crates.io, fixadas no `Cargo.lock`, e ficam fora de `gba-core` e do runtime do jogo.
- O GPIO usa a interface de caractere do kernel Linux (`/dev/gpiochipN`, uAPI v2), sem bibliotecas externas.

## Requisitos normativos

- EXT-64-01: `gba-settings --settings PATH [--page video|audio|controls|gpio]` MUST editar o mesmo `settings.toml` (SET-63-01) com abas Vídeo, Áudio, Controles e GPIO, rascunho com Aplicar/Cancelar/Restaurar padrões e escrita atômica. Arquivo inválido é preservado e informado (UI-07).
- EXT-64-02: o jogo MUST abrir a janela pelo menu de contexto e pelo menu SDL ("Configurações…"), procurando `gba-settings` ao lado do executável e depois no `PATH`; se não existir, informa e mantém o menu SDL. O jogo MUST detectar mudança do arquivo (verificação de mtime no máximo a cada 0,5 s de tempo host) e aplicar as configurações sem reiniciar, sem alterar a simulação (SET-63-02).
- EXT-64-03: o pacote `aot-compatible` MUST incluir `gba-settings` quando disponível no build.
- DEV-64-01: a aba Controles MUST listar os dispositivos conectados com nome e conexão (com fio/USB, sem fio/Bluetooth, desconhecida conforme a SDL), atualizar a lista em hotplug, e capturar bindings de teclado, de gamepad padronizado (nomes SDL) e de joystick genérico (`bN` botão, `hN<dir>` hat, `aN+`/`aN-` eixo), com Esc/Cancelar mantendo o binding anterior.
- DEV-64-02: bindings de joystick genérico (`joy_<botão>` no `settings.toml`) MUST valer para joysticks que a SDL não reconhece como gamepad (ex.: encoders arcade USB); eixos usam a zona morta global.
- GPIO-64-01: no Linux, `[gpio]` define `enabled`, `chip` (padrão `/dev/gpiochip0`), `active_low`, `bias` (`pull-up`, `pull-down`, `disabled`), `debounce_ms` (0–100) e uma linha por botão GBA (`line_a = 17`, …; ausente = sem pino). O jogo MUST requisitar as linhas como entrada pela uAPI v2 e amostrar os valores uma vez por frame; erro de abertura ou permissão é reportado e não impede o jogo. Fora do Linux a aba mostra que GPIO não está disponível.
- GPIO-64-02: a aba GPIO MUST permitir testar os pinos (mostrar nível lido ao vivo) antes de salvar, quando o jogo não estiver usando o chip.

## Fora de escopo

Interrupções/eventos de borda do GPIO (amostragem por frame basta para 60 Hz), multiplayer/atribuição de jogador, rumble, mapeamento por GUID de dispositivo.

## Critérios de aceite

Testes do modelo (`settings.toml` com `[gpio]` e `joy_*`), da conversão de nomes de tecla egui→SDL e do parser de bindings de joystick; `cargo check` do módulo GPIO para `aarch64-unknown-linux-gnu`; smoke da janela externa no macOS (abre, aplica, o jogo recarrega). Verificação em Raspberry Pi real registrada como pendente até ser executada.

## Referências

[SPEC-009](SPEC-009-settings-menu.md), [SPEC-063](SPEC-063-recompiled-frontend-settings.md), Linux `include/uapi/linux/gpio.h` (uAPI v2).
