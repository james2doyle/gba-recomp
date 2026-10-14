# SPEC-002 — CPU ARMv4T e estado arquitetural

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Decodificar ARM de 32 bits e Thumb de 16 bits do ARM7TDMI e preservar resultado, flags, modos e efeitos. Thumb-2, ARMv5 e posteriores não pertencem ao conjunto aceito.

## Requisitos normativos

- CPU-18: Thumb SWI #1 só é aceito com HLE versionada e efeitos descritos em [SPEC-023](SPEC-023-thumb-swi-register-ram-reset.md). Outros imediatos SWI e transições BIOS sem contrato continuam explicitamente recusados.
- CPU-19: ADD/SUB Thumb Format 2, com `imm3` ou registrador, é suportado somente nos limites de [SPEC-026](SPEC-026-thumb-add-sub-format-2.md), incluindo atualização de NZCV e carry de SUB como ausência de borrow.
- CPU-20: Thumb `STRH` imediato Format 10 só é suportado nos limites de [SPEC-027](SPEC-027-thumb-strh-immediate.md); isso não estende o suporte a outros loads/stores nem ao bus geral.
- CPU-21: Thumb `STRB` imediato Format 9 só é suportado nos limites de [SPEC-028](SPEC-028-thumb-strb-immediate.md); isso não estende o suporte a outros loads/stores nem ao bus geral.
- CPU-22: Thumb `LDRB` imediato Format 9 só é suportado nos limites de [SPEC-029](SPEC-029-thumb-ldrb-immediate.md), com zero-extension e leitura restrita a EWRAM/IWRAM físicas; isso não estende o suporte a outros loads/stores nem ao bus geral.

- CPU-01: cada instrução MUST manter endereço, bytes de origem, modo ISA e tamanho. Decoder MUST distinguir instrução definida, encoding indefinido e família não implementada; nunca reinterpretar extensão moderna como instrução suportada.
- CPU-02: estado MUST conter r0–r15, CPSR, bancos de registradores e SPSRs aplicáveis a User/System, IRQ, SVC, FIQ, Abort e Undefined. Não usar SPSR inexistente em User/System.
- CPU-03: leitura/escrita de PC MUST ter semântica por instrução: pipeline ARM/Thumb, alinhamento dos literais Thumb, PC em stores e shifts por registrador, BL Thumb em duas metades e retorno de exceção. `BX` usa bit 0 para modo; outras escritas de PC não podem receber automaticamente a mesma regra.
- CPU-04: ALU MUST operar módulo 2^32, com N/Z/C/V explícitos; carry em subtração significa ausência de borrow. Predicação falsa MUST preservar efeitos arquiteturais não executados e contabilizar fetch/tempo definidos.
- CPU-05: shifter MUST cobrir imediato versus registrador, 0, 1, 31, 32, >32, bits baixos do registrador, RRX e carry preservado/atualizado. Multiplicação e multiply-long MUST preservar signedness e custos relevantes.
- CPU-06: loads/stores MUST passar pelo modelo guest para desalinhamento, rotação, extensão de sinal, open bus e writeback. LDM/STM, lista vazia e base na lista exigem casos próprios; não herdar comportamento do host.
- CPU-07: transições SWI/IRQ/Undefined e retorno MUST preservar bancos, endereço de retorno, masks e CPSR/SPSR conforme contrato. Famílias sem semântica validada MUST falhar com `unsupported_instruction` antes de gerar código silenciosamente incorreto.
- CPU-08: estado ARM bancado e `MSR CPSR` seguem o contrato incremental da [SPEC-013](SPEC-013-banked-cpu-and-psr.md); não alterar layout do ABI v1 para acomodá-los.
- CPU-09: carga literal ARM por PC pode ser fixada em constante AOT somente nos limites imutáveis da [SPEC-014](SPEC-014-rom-literal-loads.md); outras cargas passam pelo bus ou são recusadas.
- CPU-10: `ADD Rd, PC, #imm` ARM sem flags é suportado somente nos limites da [SPEC-015](SPEC-015-arm-pc-relative-add.md), com valor PC visível de `guest_pc+8`; nenhuma outra forma de uso de PC é inferida desse caso.
- CPU-11: escrita ARM `STR` de palavra com offset imediato só é suportada nos limites da [SPEC-016](SPEC-016-ram-word-stores.md). Ela passa pela ABI GBA de memória; aceitação desse formato não implica suporte aos demais loads/stores nem ao bus completo.
- CPU-12: `MOV Rd,Rm` ARM com shifter `LSL #0`, sem flags e sem destino PC, só é suportado nos limites da [SPEC-017](SPEC-017-arm-mov-register.md). A leitura de `Rm=PC` usa `guest_pc+8`; isso não autoriza outros shifts nem escrita de PC.
- CPU-13: `BX Rm` e o despacho por modo são suportados somente nos limites da [SPEC-018](SPEC-018-arm-thumb-branch-exchange.md). O modo de destino vem do bit 0 de `Rm`, e destinos sem fechamento AOT estático são recusados.
- CPU-14: Thumb `PUSH {rlist}` com r0–r7 e LR é suportado somente nos limites da [SPEC-019](SPEC-019-thumb-push.md). Efeitos de stack passam pela ABI GBA v3 e são limitados às regiões físicas aceitas por esse contrato.
- CPU-15: Thumb `MOV Rd,Rm` high-register é suportado somente nos limites da [SPEC-020](SPEC-020-thumb-high-mov.md). `Rm=PC` usa `guest_pc+4`; escrita em PC e outras operações high-register continuam recusadas.
- CPU-16: Thumb `MOVS Rd,#imm8` é suportado somente nos limites da [SPEC-021](SPEC-021-thumb-movs-immediate.md), incluindo N/Z e preservação de C/V e dos demais bits de CPSR.
- CPU-17: `BL` Thumb de duas metades e o retorno por `BX LR` seguem a [SPEC-022](SPEC-022-thumb-bl-pair.md), com pareamento adjacente, LR e destino fechados estaticamente, sem clobbers presumidos de ABI host.

## Interface

`decode(address, mode, bytes) → DecodedInstruction`; `lift(instruction, state) → GbaIr`. Matriz de famílias obrigatória: data processing, PSR transfer, multiply/long, swap, single/halfword/signed transfer, block transfer, branch/BX/BL, SWI e grupos Thumb equivalentes. Coprocessor/encodings reservados ficam classificados com comportamento Undefined ou erro técnico conforme evidência, nunca NOP implícito.

## Aceite e matriz

| Dimensão | Verificação exigida |
|---|---|
| ARM condicional | Todas as condições × combinações NZCV |
| Flags/overflow | zero, limites signed/unsigned, carry/borrow |
| Shifts | Casos enumerados em CPU-05 por modo |
| PC e interworking | ARM↔Thumb, literais, BL/retorno, IRQ |
| Memória | 8/16/32 bits, cada alinhamento e writeback |
| Bancos | IRQ/SWI aninhados permitidos e retorno |
| Decoder | Corpus válido/indefinido e disassembler independente |

## Observabilidade e questões

Trace inclui PC/modo/instrução, diferenças de registradores e ciclos. A cobertura é por família/caso, não apenas percentual de opcodes. Timing detalhado depende da SPEC-005. Fontes: KNO-001 SRC-09 e mGBA `src/arm/`; casos imprevisíveis exigem evidência antes de promoção a suporte.
