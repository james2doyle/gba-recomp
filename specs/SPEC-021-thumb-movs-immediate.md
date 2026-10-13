# SPEC-021 — MOVS Thumb imediato em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar somente o formato Thumb `MOVS Rd,#imm8` (Format 3, opcode `00`) da ARMv4T, observado na próxima fronteira estática de LeafGreen após `PUSH` e `MOV` high-register. `CMP`, `ADD` e `SUB` com imediato, assim como outros formatos ALU Thumb, ficam fora desta spec.

## Requisitos normativos

- TMOVI-01: reconhecer o encoding quando `encoding & 0xF800 == 0x2000`; extrair `Rd = bits[10:8]` (r0–r7) e `imm8 = bits[7:0]`. Outros opcodes do grupo Format 3 devem continuar recusados.
- TMOVI-02: escrever `zero_extend(imm8)` em Rd. Limpar N, definir Z quando `imm8 == 0`, preservar C e V e todos os demais bits de CPSR. Não mudar o modo Thumb. Avançar PC em dois bytes e contabilizar uma unidade abstrata.
- TMOVI-03: decoder, IR, descoberta e LLVM AOT devem preservar os efeitos de TMOVI-02. Descoberta pode propagar `Rd=imm8` como constante. A execução estritamente AOT não pode cair em interpretador ou JIT.
- TMOVI-04: testar immediatos `0xFF` e `0` em O0/O2 depois de estabelecer C=1 e V=1. O primeiro deve deixar N/Z limpos; o segundo deve deixar N limpo e Z definido. Ambos devem preservar C/V e o estado do modo.
- TMOVI-05: testar rejeição de `CMP`, `ADD` e `SUB` imediatos Thumb, além da ausência de pacote parcial para instruções alcançáveis não suportadas.

## Evidência e limites

A compilação estática da ROM LeafGreen identificada em KNO-004 chegou a `0x080003AA`, raw Thumb `0x20FF`, depois de suportar as fronteiras anteriores. A leitura como `MOVS r0,#0xFF` é inferência dos campos do encoding, corroborada pelas observações de fonte em SRC-22. Nenhum pacote LeafGreen foi criado e o jogo não foi executado. As fixtures sintéticas provarão apenas o formato e os efeitos especificados aqui.
