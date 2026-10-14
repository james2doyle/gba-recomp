# SPEC-060 — BIOS HLE, backup do cartucho e estado inicial do núcleo

Estado do documento: Accepted em 2026-10-14. Escrita junto com a primeira implementação (ADR-041). Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Aplicar BIOS-01, CART-01 e SAVE-01 da [SPEC-007](SPEC-007-bios-cart-state.md) ao núcleo interpretado: BIOS HLE versionada ou BIOS real fornecida localmente, backups SRAM/Flash/EEPROM e persistência atômica.

## Requisitos normativos

- HLE-60-01: com HLE (`HLE_VERSION` 1) o núcleo inicia no estado pós-boot (System, pilhas 0x03007F00/0x03007FA0/0x03007FE0, PC 0x08000000, SOUNDBIAS 0x200, POSTFLG 1). Com BIOS fornecida, inicia no vetor de reset e executa o código real.
- HLE-60-02: o vetor IRQ MUST ser código ARM real na região BIOS (salva r0–r3/r12/lr, chama [0x03FFFFFC], retorna com `SUBS pc, lr, #4`), para que o contrato do handler do usuário seja o do hardware.
- HLE-60-03: SWIs implementados: 0x00–0x19, 0x1F, 0x25 (falha documentada: sem parceiro de link), 0x27. Qualquer outro MUST parar com `UnsupportedSwi{number, pc}`. IntrWait/VBlankIntrWait MUST usar HALT e reexecutar a chamada após IRQ até o flag em 0x03007FF8 estar presente.
- HLE-60-04: rotinas de descompressão MUST respeitar a largura de escrita (variantes VRAM usam halfwords). ArcTan/ArcTan2 usam o algoritmo inteiro documentado; BgAffineSet/ObjAffineSet usam tabela de seno de 256 entradas; MidiKey2Freq usa ponto flutuante host (limitação registrada para VAL-04).
- CART-60-01: tipo de backup detectado por assinaturas `EEPROM_V`, `FLASH1M_V`, `FLASH512_V`/`FLASH_V`, `SRAM_V`/`SRAM_F_V`; override explícito permitido. EEPROM resolve 512 B/8 KiB pelo tamanho do primeiro DMA (9/73 ou 17/81 bits).
- CART-60-02: Flash MUST implementar sequência de comandos AA/55, ID (Macronix C2/09 para 128 KiB, Panasonic 32/1B para 64 KiB), erase de chip/setor 4 KiB, program byte (AND) e troca de banco. EEPROM MUST implementar leitura (4 bits dummy + 64) e escrita de 64 bits com status pronto.
- SAVE-60-01: o host MUST persistir o backup só quando sujo, escrevendo arquivo temporário e renomeando.

## Limites

Sem RTC/GPIO (Ruby/Sapphire/Emerald precisarão), sem savestate (STATE-01). Os IDs Flash são escolha local e devem constar do relatório.

## Critérios de aceite

Testes unitários de compressão (LZ77/RL/Huffman/Diff), Div/Sqrt/ArcTan2, protocolo Flash e EEPROM; LeafGreen grava e relê save (KNO-007).

## Verificação

Testes em `crates/gba-core/src/bios/tests.rs` (21), `backup/tests.rs` (11) e `gba/tests.rs` (5), 2026-10-14: Div/DivArm com sinais e overflow, Sqrt, ArcTan/ArcTan2 contra avaliação independente da fórmula GBATEK nos quatro quadrantes, CpuSet/CpuFastSet (cópia/fill 16/32, arredondamento a 8 words), LZ77 (WRAM em bytes com destino ímpar vs VRAM em halfwords), RL, Huffman 4/8 bits, Diff8/Diff16, BitUnPack (offset, flag de zero, 8→32), BgAffineSet/ObjAffineSet (identidade, 90°/180°, stride 2/8), RegisterRamReset por flag, SoftReset, IntrWait/VBlankIntrWait (descarte, HALT, reexecução até o flag em 0x03007FF8), SWIs simples e `UnsupportedSwi`; backup: detecção por assinatura, SRAM, Flash (modo ID 64/128 KiB, program com AND, erase de setor/chip, troca de banco), EEPROM 512 B/8 KiB por acessos de 16 bits e por DMA com inferência de tamanho; sistema: ROMs ARM montadas à mão contando frames pelo stub IRQ da BIOS e por VBlankIntrWait, e HALT com IME=0. MidiKey2Freq e a persistência atômica do host (SAVE-60-01, no `gba-run`) não têm teste aqui; a gravação de save na LeafGreen continua pendente. Bugs corrigidos: [KNO-007](../knowledge/KNO-007-interpreted-core.md#bugs-corrigidos-pelos-testes-unitários).

## Referências

[SPEC-007](SPEC-007-bios-cart-state.md), [SPEC-023](SPEC-023-thumb-swi-register-ram-reset.md), GBATEK.
