# SPEC-022 — BL Thumb de duas metades em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar o par Thumb `BL` da ARMv4T: metade alta que prepara LR e metade baixa que chama um destino estático. Manter as duas meias-instruções distintas na IR e na contagem abstrata. `BLX`, branches Thumb condicionais/incondicionais e chamadas cujo destino não possa ser fechado estaticamente ficam fora desta spec.

## Requisitos normativos

- TBL-01: reconhecer o prefixo quando `encoding & 0xF800 == 0xF000`. Interpretar `imm11` com sinal de 11 bits e calcular `lr_partial = guest_pc + 4 + (sign_extend(imm11) << 12)`, módulo 2^32. O prefixo grava LR, avança PC em dois bytes, permanece em Thumb e conta uma unidade abstrata.
- TBL-02: reconhecer o sufixo quando `encoding & 0xF800 == 0xF800`, somente no endereço imediatamente posterior ao prefixo (`prefix_pc+2`). Calcular `target = lr_partial + ((imm11 & 0x7FF) << 1)`. O sufixo grava `LR = (guest_pc+2)|1`, avança o PC ao destino Thumb, mantém CPSR.T e conta uma unidade abstrata.
- TBL-03: descoberta deve validar o par, provar que o LR de entrada ao sufixo é exatamente o produzido pelo prefixo adjacente e rejeitar sufixo isolado, par quebrado, target desalinhado ou fora da ROM. O alvo de chamada deve existir no fechamento AOT; nenhum fallback de interpretação/JIT é permitido.
- TBL-04: manter prefixo e sufixo como duas instruções guest na IR, no PC, nos ciclos abstratos e no manifesto. Código LLVM pode materializar o destino constante depois que a descoberta provar a dependência de LR.
- TBL-05: `BX LR` mantém o estado de registradores guest que a própria execução alcançou; a descoberta segue a aresta estática e mescla os estados no destino. Não aplicar clobbers presumidos de ABI host nem supor uma convenção de chamadas não observada.
- TBL-06: testar chamada Thumb, execução do corpo e retorno `BX LR` até o continuation em O0/O2, verificando LR, PC, modo e estado observável. Testar extensão negativa de `imm11`, sufixo sem prefixo, par interrompido, destino fora da ROM e falha sem pacote parcial.

## Evidência e limites

Na ROM LeafGreen de KNO-004, a descoberta parou em `0x080003AC` (`0xF1E3`); o halfword seguinte, lido estaticamente em `0x080003AE`, é `0xFC0E`. A classificação como par BL é inferência pelos campos, corroborada pelas observações de fonte SRC-23. Aplicando as regras da referência, o LR parcial é `0x081E33B0` e o destino calculado é `0x081E3BCC`, dentro da imagem identificada de 16 MiB. Não houve pacote LeafGreen nem execução do jogo.
