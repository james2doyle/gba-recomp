# ADR-023 — Helper aditivo para Thumb STMIA na ABI v3

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

SPEC-036 acrescenta STMIA Thumb T1 para até oito registradores r0–r7. O helper atual `gba_store32_v3` aceita uma palavra alinhada por chamada. Repeti-lo no LLVM expõe commits parciais se uma transferência posterior falhar e torna difícil manter writeback e base incluída na lista coerentes. Layouts ABI v2/v3/v4 não devem mudar.

O ARM ARM não garante atomicidade all-or-nothing de todas as words em Data Abort; por isso o helper novo oferece atomicidade somente para faults locais do subset EWRAM/IWRAM, como política do recompiler.

## Decisão

Adicionar `gba_stmia_thumb_v3(rt,out,base_register,register_list)` como símbolo ABI v3 aditivo. O helper valida a ABI, a lista e a faixa inteira antes de escrever. Captura os registradores selecionados em temporários, armazena as words little-endian em ordem crescente em uma única região EWRAM/IWRAM física e então atualiza o Rb para `old_Rb + 4*popcount(list)`, com wrap u32.

Os endereços de cada word alinham para baixo; o cálculo de writeback usa o endereço-base original e preserva seus bits baixos. Lista vazia e Rb incluso depois de um membro menor são rejeitados como casos arquiteturalmente imprevisíveis. Quando Rb é o menor selecionado, a memória recebe seu valor original, seguido de writeback no registrador.

Discovery atualiza a abstração do Rb. Se a base singleton e todo o intervalo forem RAM física, a sombra da SPEC-034 recebe os valores abstratos pré-writeback correspondentes; caso contrário, a sombra inteira é limpa. Fault local retorna `GbaExitV3` sem alterar memória ou CPU; lowering avança PC/ciclo somente após sucesso. O ciclo segue abstrato (+1), não timing do GBA.

## Alternativas

- Chamar `gba_store32_v3` uma vez por palavra: rejeitada porque o primeiro store poderia ser visível antes de uma falha posterior e cada chamada poderia validar um subconjunto distinto.
- Alterar estruturas ABI existentes para encapsular arrays/bus: rejeitada por quebrar compatibilidade sem resolver mapeamento geral.
- Alinhar/rejeitar qualquer base desalinhada: rejeitada porque a semântica ARM7TDMI ignora os bits baixos do EA para Store Multiple, mas os preserva no writeback.
- Definir atomicidade local como Data Abort arquitetural: rejeitada; os manuais não fornecem essa garantia.
- Interpretar ou usar JIT para regiões não cobertas: rejeitada no modo strict-AOT.

## Consequências

O helper contém o commit em um único ponto e mantém faults locais inspecionáveis em `GbaExitV3`; ainda assim, não fornece bus geral, espera física ou semântica completa de Data Abort. A sombra de discovery fica mais precisa para stores com endereço e valores singleton, sem inferir valores guest desconhecidos. Vendor refs permanecem read-only; não foi copiado código.

## Critérios para revisitar

- Implementação de bus versionado geral e timing físico na SPEC-005.
- Evidência diferencial de STMIA em hardware/oráculo para regiões não cobertas.
- Necessidade comprovada de política de store múltiplo que não caiba no helper de lote.

## Referências

[SPEC-036](../specs/SPEC-036-thumb-stmia.md), [SPEC-034](../specs/SPEC-034-abstract-memory-discovery.md), [ADR-020](ADR-020-additive-thumb-pop-abi.md), [KNO-001 SRC-36](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
