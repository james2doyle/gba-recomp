# SPEC-005 — Barramento, scheduler, DMA, timers e IRQ

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Reproduzir efeitos temporais observáveis do GBA independentemente da velocidade da CPU host.

## Mapa base

| Base | Região | Capacidade física de referência |
|---|---|---|
| 0x00000000 | BIOS | 16 KiB |
| 0x02000000 | EWRAM | 256 KiB |
| 0x03000000 | IWRAM | 32 KiB |
| 0x04000000 | MMIO | Contratos por registrador |
| 0x05000000 | Palette RAM | 1 KiB |
| 0x06000000 | VRAM | 96 KiB |
| 0x07000000 | OAM | 1 KiB |
| 0x08000000–0x0DFFFFFF | Game Pak, três janelas | Até 32 MiB compartilhados |
| 0x0E000000 | Backup | Conforme cartucho |

## Requisitos normativos

- BUS-01: bus MUST implementar por região mirrors, permissões/efeitos, widths, desalinhamento, open bus, latch BIOS, writes byte VRAM/palette/OAM e espaços não mapeados. A tabela acima não autoriza módulo uniforme em todas as regiões.
- TIME-01: relógio guest MUST usar ciclos inteiros de 64 bits, com frequência nominal 16.777.216 Hz; frame nominal 280896 ciclos. Monotonicidade e overflow devem ser verificados. Timestamps host não determinam ordem de eventos guest.
- TIME-02: custos MUST depender do estado pertinente: sequencial/não sequencial, WAITCNT, prefetch, largura/região, refill de pipeline, transferências e stalls DMA. Não assumir WAITCNT típico de um jogo como verdade global.
- TIME-03: antes de efeito observável, runtime MUST avançar os periféricos até o instante correspondente. Bloco que atravessa deadline MUST ter checkpoints adequados; instruções/bus multicíclo processam eventos durante seu intervalo conforme o modelo, e IRQ entra na fronteira arquitetural apropriada.
- TIME-04: eventos simultâneos MUST ter ordenação documentada e respaldada em fixture por conflito. Baseline avança PPU/timers/APU/serial ao ciclo do evento, arbitra DMA e então reavalia IRQ na fronteira CPU; prioridades reais e latências são gates de conformance, não a ordem acidental de containers.
- DMA-01: quatro canais MUST modelar prioridade, start imediato/HBlank/VBlank/especial, repeat, incremento/decremento/fixo/reload, largura 16/32, count zero, bloqueio CPU, IRQ e invalidação de código. Atalhos memcpy não podem pular MMIO, evento ou latch.
- TIM-01: quatro timers MUST modelar prescaler, reload, overflow, cascade e conexão FIFO/IRQ. Writes são aplicados no ciclo definido.
- IRQ-01: IE/IF/IME e CPSR masks MUST controlar pending/delivery; IF write-one-to-clear, banking e retorno são explícitos. HALT/STOP MUST ter regras próprias de wake e consumo; não ocupar host em busy loop.
- TIME-05: otimização idle-loop MAY saltar até próximo evento somente com prova de polling, ausência de efeitos omitidos e condição de wake. Um perfil não pode simplesmente ignorar MMIO para ganhar FPS.

## Interfaces e observabilidade

`advance_to(cycle)`, `next_deadline()`, `bus_access(master, address, width, kind)`. Trace versionado: timestamp, ator CPU/DMA/periférico, address/value, IRQ, eventos e estado de prefetch relevante. Históricos limitados por tamanho e exportados em falha. `unsupported_hardware` deve explicar registrador e capacidade ausente.

## Aceite e matriz

Mirrors de RAM/VRAM/ROM; open bus e acesso BIOS fora da BIOS; cargas desalinhadas; WAITCNT mudado durante execução; DMA entre regiões e para código; timers simultâneos/cascade; HBlank/VBlank com polling; IRQ habilitada/limpa na fronteira; HALT com evento pendente. Comparar trace AOT e referência com mesma fixture. Nenhum checkpoint pode atrasar MMIO silenciosamente pelo tamanho de bloco escolhido pelo LLVM.

## Questões e fontes

Precisão fina e eventos simultâneos devem ser resolvidos por testes antes da alegação de fidelidade. KNO-001 SRC-04–09; mGBA `src/gba/{memory,dma,timer,gba}.c`.
