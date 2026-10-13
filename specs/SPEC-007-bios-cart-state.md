# SPEC-007 — BIOS, cartucho, saves e estado

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Completar hardware necessário ao jogo e persistir progresso sem confundir backup do cartucho com savestate do runtime.

## Requisitos normativos

- BIOS-01: perfil MUST escolher HLE versionado ou BIOS fornecida localmente e identificada por hash. HLE MUST declarar SWIs implementadas, efeitos em registradores/memória, ciclos e contrato de IRQ/IntrWait. SWI não implementada é erro explícito; nunca retorna sucesso vazio.
- BIOS-02: modo BIOS real MUST respeitar reset, exceções, proteção de leitura/open bus e cobertura de código. Em strict-aot, código BIOS executado também MUST estar pré-compilado. Assets BIOS não são incluídos implicitamente no pacote.
- CART-01: SRAM, Flash 64/128 KiB e EEPROM 512 B/8 KiB MUST ter protocolos definidos. Scans de assinaturas são hints; override por hash resolve ambiguidade e deve ser registrado. Capacidade não detectada gera pendência de perfil, não escolha silenciosa.
- CART-02: RTC, GPIO, rumble, solar/tilt, serial/link, e-Reader e mappers especiais MUST aparecer no relatório de capacidades. Implementar RTC e serial desconectado no baseline; demais capacidades exigem spec incremental antes de declarar suporte. Ler serial desconectado deve reproduzir esse estado, não sucesso fictício de link.
- SAVE-01: backup MUST persistir por identidade de jogo e formato/versão, em escrita atômica com recuperação após falha. Compartilhar save entre hashes requer seleção explícita e compatibilidade validada. Trocar tipo de backup exige migração, cópia e confirmação no fluxo de usuário.
- STATE-01: savestate MUST incluir CPU/bancos/pipeline, RAM, periféricos, scheduler, DMA/timers, FIFOs, backup em sessão, RTC determinístico e identidade de ROM/BIOS/perfil/ABI/estado. Ponteiros host, threads, handles e código LLVM não podem ser serializados.
- STATE-02: restauração MUST rejeitar identidade/versão incompatível, reconstruir dispatch e invalidar código RAM. Load não pode carregar tradução stale nem sobrescrever backup persistido sem política declarada. Baseline: backup restaurado marca sessão dirty e só persiste no próximo commit explícito/encerramento normal.
- RTC-01: verificação MUST usar epoch fixo e avanço por tempo guest. Modo relógio real MAY existir como configuração semântica, devendo ser registrado no replay; não é determinístico por padrão.

## Interfaces, erros e aceite

`BackupStore`, `CartridgeCapabilities`, `BiosPolicy`, `StateEnvelope`. Erros: `bios_required`, `swi_unsupported`, `backup_ambiguous`, `state_incompatible`, `save_io_failed`.

Matriz: SWIs usadas pela fixture com casos de borda; Flash commands/bancos; EEPROM via DMA e widths; save interrompido no meio de write; reload de estado com DMA/FIFO ativo; ROM errada; RTC fixo; periférico ausente. Estado restaurado MUST reproduzir trace futuro com mesma entrada.

## Referências e questões

mGBA `bios.c`, `savedata.c`, `sio.c`, `serialize.c`, cart GPIO; KNO-001 SRC-08. Lista inicial de SWIs é gate da fixture, não promessa de BIOS completa.
