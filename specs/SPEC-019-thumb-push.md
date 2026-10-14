# SPEC-019 — PUSH Thumb para RAM física em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar o formato Thumb `PUSH {rlist}` e `PUSH {rlist,lr}` identificado após BX na descoberta estática de LeafGreen. O suporte cobre somente os registradores r0–r7 e LR, com stack em EWRAM/IWRAM físicas por meio da ABI GBA v3. `POP`, demais transferências múltiplas, mirrors e MMIO ficam fora deste contrato.

## Requisitos normativos

- TPUSH-01: reconhecer Thumb `1011 010 L rlist8`, máscara `encoding & 0xFE00 == 0xB400`. Os bits 7–0 selecionam r0–r7; bit 8 inclui LR. Rejeitar uma lista vazia e qualquer encoding fora desse formato.
- TPUSH-02: o efeito guest é `STMDB SP!, {regs}`. Capturar os valores selecionados antes das escritas; calcular `new_sp = old_sp - 4 * count` com aritmética de 32 bits; armazenar registradores em ordem crescente, do menor endereço em `new_sp`, com LR por último quando selecionado; escrever `SP = new_sp` após sucesso. CPSR não muda, PC avança dois bytes e a instrução conta uma unidade abstrata.
- TPUSH-03: o código emitido chama a helper da ABI GBA v3 para este efeito de memória. A ABI pode ganhar uma função, mas os layouts v1–v3 não mudam. Endereços guest continuam inteiros e nunca são convertidos em ponteiros host pelo código guest.
- TPUSH-04: a helper aceita somente intervalos inteiros e alinhados contidos em EWRAM física (`0x02000000..0x02040000`) ou IWRAM física (`0x03000000..0x03008000`). Deve validar todo o intervalo antes de escrever qualquer palavra. Em falha, memória, SP, PC e ciclos permanecem sem mutação; `GbaExitV3` registra razão, PC, endereço guest e fault code.
- TPUSH-05: descoberta segue para `guest_pc+2`; a análise propaga `SP - 4 * count` somente quando o SP de entrada é estaticamente limitado e torna SP desconhecido nos demais casos. Quando os requisitos de [SPEC-034](SPEC-034-abstract-memory-discovery.md) forem satisfeitos, também registra os valores salvos para uso por POP; essa sombra não presume endereço nem conteúdo inicial de RAM. Nenhuma instrução não suportada pode cair silenciosamente em interpretação ou JIT.
- TPUSH-06: testar `PUSH {r4-r7,lr}` em O0/O2, ordem dos valores, SP e CPSR; cobrir alinhamento e fronteira de região; endereço inválido não pode deixar pacote parcial.

## Evidência e limites

LeafGreen parou em `0x080003A4`, raw Thumb `0xB5F0`. A classificação como `PUSH {r4-r7,lr}` é inferência do encoding, corroborada pelas observações SRC-20 das referências fixadas. Não houve pacote LeafGreen nem execução do jogo. A fixture sintética prova apenas o formato e as regiões cobertas neste contrato.
