# SPEC-009 — Menu de vídeo, áudio e controles

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Configurar o executável nativo por teclado e gamepad, antes e durante o jogo. O menu deve funcionar sem ferramentas de compilação.

## Páginas e semântica

| Página | Controles obrigatórios | Default proposto | Aplicação |
|---|---|---|---|
| Vídeo | janela/tela cheia, escala inteira, proporção, nearest/linear, cor, persistência LCD, vsync | janela, 3× quando couber, 3:2, nearest, cor/persistência off, vsync on | Próximo frame; troca de backend exige reinício |
| Áudio | dispositivo, volume 0–100%, mute, buffer/latência 20–200 ms | sistema, 100%, mute off, 60 ms | Próximo chunk ou reabertura do dispositivo |
| Controles | A/B/L/R/Start/Select/direcional, teclado/gamepad, remap, deadzone, atalhos | teclado e gamepad padrão | Próximo ponto de amostragem input |
| Jogo | global versus override, reset por página, perfil/revisão, relatório | herdar | Conforme categoria |

Taxa de áudio usa valor suportado pelo dispositivo; sugestão 48 kHz, fallback negociado exibido. Escala que excede a tela deve ajustar para caber. Intervalos representam contrato de UI, não promessa de latência real.

## Requisitos normativos

- UI-01: menu MUST abrir antes do jogo e durante execução por atalho reconfigurável, navegar totalmente com teclado/gamepad, exibir foco, suportar cancel/back e labels legíveis em escala de DPI. Remap MUST mostrar ação aguardada e permitir cancelar sem perder binding anterior.
- UI-02: alteração MUST ficar em rascunho até Aplicar; Cancelar restaura valor efetivo. Reset por jogo remove override; reset global volta ao default após confirmação de escopo. Persistência é atômica e migra schema preservando backup.
- UI-03: opções MUST informar escopo, valor efetivo e origem. Mudanças de compilação mostram “requer recompilar”; hardware/BIOS/backup mostram “requer reiniciar”. Aplicar não pode fingir efeito imediato nesses casos.
- UI-04: modo de vídeo arriscado MUST ter reversão automática em 15 segundos de tempo host se não confirmado, inclusive com jogo pausado. Falha ao recriar janela preserva preferências anteriores.
- UI-05: áudio MUST permitir mute sem parar APU, hotplug com fallback ao dispositivo padrão e indicação de underruns. Dispositivo não encontrado não impede iniciar o jogo.
- INPUT-01: input MUST ser convertido em dez botões GBA e KEYINPUT active-low; KEYCNT IRQ e instante de amostragem pertencem ao runtime. Direções opostas usam política explícita (baseline neutralizar pares), deadzone entre 0 e 0,95 e histerese documentada.
- INPUT-02: gamepads MUST ser identificados por ID estável quando disponível; desconexão libera botões, mantém bindings e informa estado. Perda de foco libera teclas. Atalho do menu é consumido pela UI para evitar acionamento acidental guest.
- INPUT-03: turbo e macros são opcionais avançados, desativados no baseline, registrados em replay como input efetivo. Controles touchscreen pertencem ao adaptador mobile, sem assumir teclado disponível.
- UI-06: menu MUST pausar guest por padrão e silenciar/drenar apresentação; retornar mantém relógio guest e evita acumular tempo host como dívida de emulação. Retomar requer estado de input atualizado.
- UI-07: configuração inválida MUST preservar arquivo original, usar defaults seguros e mostrar diagnóstico. Escrita impossível é informada; não declarar configuração salva.

## Formato, erros e aceite

`settings.toml`: `version=1`, tabelas `video`, `audio`, `input`; overrides por SHA-256 em arquivo próprio. Campos técnicos de perfil são rejeitados nesse arquivo. Separar preferências de máquina (IDs de dispositivo) de bindings compartilháveis.

Matriz: navegação sem mouse, remap/cancel/conflito, hotplug, foco, mudança de DPI/fullscreen com rollback, mute com FIFO ativo, restauração de defaults, herança global/jogo, arquivo inválido/read-only e persistência após relançar. Hash guest com mesmos inputs MUST permanecer idêntico ao variar vídeo/volume/latência.

## Referências e questões

KNO-001 SRC-10; biblioteca gráfica exata e acessibilidade nativa dependem do adaptador (GBA-070). Não expor detalhes LLVM no fluxo básico.
