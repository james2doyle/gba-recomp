# SPEC-020 — MOV Thumb entre registradores high em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar o subformato Thumb `MOV Rd,Rm` de registradores low/high, necessário para a instrução observada depois do PUSH de LeafGreen. A operação não altera flags nem o modo. Outras operações high-register (`ADD`, `CMP`) e escrita de PC ficam fora desta tranche.

## Requisitos normativos

- THMOV-01: reconhecer o grupo high-register `encoding & 0xFC00 == 0x4400` quando o opcode em bits 9–8 é `10` (MOV). Decodificar `Rd = bits[2:0] | (H1 << 3)` e `Rm = bits[6:3]`; aceitar `Rd=r0-r14` e `Rm=r0-r15`. Rejeitar escrita em PC e os outros opcodes do grupo.
- THMOV-02: copiar o valor de `Rm` para `Rd` sem alterar CPSR; `Rm=PC` lê `guest_pc+4`, os demais registradores vêm do estado guest atual. PC avança dois bytes e o custo abstrato é uma unidade.
- THMOV-03: decoder, IR, análise abstrata e LLVM AOT devem preservar os registradores high e o valor de PC conforme THMOV-02. Não há chamada a interpretador/JIT.
- THMOV-04: testar `MOV r7,r8` em O0/O2, preservar flags e rejeitar `MOV PC,Rm`; cobrir a leitura Thumb de PC se `Rm=PC` for aceita pelo decoder.

## Evidência e limites

LeafGreen parou em `0x080003A6`, raw Thumb `0x4647`. A leitura como `MOV r7,r8` é inferência pelos campos, corroborada como observação de fonte em SRC-21. A tranche não cobre o restante da família high-register.
