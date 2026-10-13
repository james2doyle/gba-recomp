# SPEC-023 — Thumb SWI #1 RegisterRamReset em AOT estrito

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e limite

LeafGreen alcança estaticamente o halfword 0xDF01 em 0x081E3BCC, inferido como Thumb SWI #1. Este contrato decodifica Thumb SWI e implementa somente RegisterRamReset pela HLE versionada. A observação é estática; não é trace de execução nem prova de que outros SWIs usados pelo jogo estejam cobertos.

## Requisitos

- SWI-01: o decoder MUST representar o imediato de 8 bits e manter PC, encoding e modo Thumb. Encodings 0xDF00–0xDFFF são SWI; 0xDE00–0xDEFF permanecem Undefined.
- SWI-02: em strict-AOT, somente imediato 0x01 MUST ser aceito. Outro imediato gera swi_unsupported durante descoberta e não publica pacote parcial. Não há trap convertido em sucesso, interpretação, JIT ou BIOS implícita.
- SWI-03: RegisterRamReset MUST ler r0 em runtime como máscara. Bits superiores e bits sem significado são ignorados; bits 0–7 têm os efeitos abaixo.
- SWI-04: a chamada HLE MUST preservar registradores CPU, CPSR, SPSRs e modo Thumb, exceto avançar PC para guest_pc + 2. Não há transição ao modo SVC: esta política modela chamada BIOS por HLE.
- SWI-05: a HLE MUST atualizar as regiões e latches listados nesta spec. Bit 0x01 zera toda EWRAM; 0x02 zera IWRAM [0x03000000,0x03007E00), preservando os últimos 0x200 bytes; 0x04 zera palette RAM; 0x08 zera VRAM; 0x10 zera OAM.
- SWI-06: com r0 & 0x20, a HLE MUST escrever SIOCNT=0, RCNT=0x8000, SIOMLT_SEND=0, JOYCNT=0, JOY_RECV=0 e JOY_TRANS=0 nos latches de I/O.
- SWI-07: com r0 & 0x40, MUST zerar os registradores de som abrangidos por SOUND1CNT_LO até SOUNDCNT_X, SOUND BIAS MUST virar 0x0200 e os 32 bytes de wave RAM MUST virar zero.
- SWI-08: com r0 & 0x80, MUST limpar os latches de vídeo, DMA, timers, IE, IF, WAITCNT e IME que RegisterRamReset escreve. Defaults affine BG2PA/BG2PD/BG3PA/BG3PD são 0x0100; DISPCNT MUST ser 0x0080 em toda chamada. DISPSTAT preserva bits de status 0–2 e limpa campos graváveis; VCOUNT permanece read-only. Escrever 0xFFFF em IF limpa os bits pendentes, cujo latch fica zero.
- SWI-09: a instrução conta como uma instrução AOT e avança uma unidade no modelo abstrato atual. Isso não afirma equivalência com ciclos de hardware. O helper MUST validar ABI, ponteiros, versões e relação entre os contextos antes de qualquer mutação; falha não altera CPU nem memória.
- SWI-10: pacotes MUST registrar abi_version=4 e bios_hle_version=1. Nenhum pacote pode depender de uma versão HLE não declarada.

## ABI v4 e estado de máquina

ABI v4 mantém GbaRuntimeV3 como primeiro membro de GbaRuntimeV4, acrescenta GbaMachineStateV1* machine e abi_version=4. GbaMachineStateV1 contém como primeiro membro o GbaMemoryV1 legado, seguido por abi_version=1, palette RAM (0x400), VRAM (0x18000), OAM (0x400), latches IO (0x400) e wave RAM (32 bytes). O ponteiro GbaRuntimeV3.memory MUST ser exatamente &machine->memory; o helper valida essa igualdade. Endereços guest continuam u32; arrays e ponteiros host nunca entram na CPU guest.

O entry point emitido é gba_block_v4(GbaRuntimeV4*, GbaExitV3*). Helpers de memória v3 recebem somente o prefixo GbaRuntimeV3. ABI v1–v3 e seus offsets permanecem congelados. O manifesto identifica a HLE RegisterRamReset v1. O estado de latches não implementa, por si só, execução de PPU/APU, apresentação, DMA, timers ou um bus geral.

## Matriz de aceite

| Caso | Resultado |
|---|---|
| Cada bit de r0 0–7 testado isoladamente | Somente os efeitos definidos para esse bit |
| Máscaras combinadas e bits altos | Efeitos combinados; bits altos sem efeito |
| IWRAM reset | 0x7E00 bytes limpos; sentinela final de 0x200 bytes preservada |
| Reset de IO | Valores/defaults e semântica de DISPSTAT, VCOUNT e IF conforme SWI-08 |
| Registradores CPU e banco atual | Inalterados, exceto PC+2; CPSR.T continua ativo |
| ABI/contexto ausente ou incompatível | Fault explícito, nenhuma mutação |
| Outro imediato SWI | Falha de descoberta; nenhum executável ou pacote parcial |
| O0 e O2 | Mesmo estado observável após a chamada |
| Package manifest | ABI e versão HLE identificados |

## Evidência, limites e referências

KNO-004 fixa a ROM em SHA-256 2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825. SRC-24 registra mGBA commit 143adf5aa2a60c6036810b0cd72445e13d74b0e6 e gba-recomp commit de4edf59b872d887046d6a3b005e2df551b6d44c. mGBA implementa os resets em src/gba/bios.c e despacha IO em src/gba/io.c; gba-recomp tem HLE parcial em crates/gba-core/src/hle.rs. Essas são observações de fonte, não execução diferencial nem validação em hardware.

A implementação local cobre somente os efeitos descritos acima. Bus geral, leitura/escrita MMIO fora da chamada, renderer, APU, timing real, backup, outros SWIs e execução LeafGreen permanecem fora deste contrato.
