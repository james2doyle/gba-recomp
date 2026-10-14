# ADR-024 — Especialização AOT de LDMIA ROM com bases finitas

- Estado: Accepted
- Data: 2026-10-14
- Supersede: —

## Contexto

SPEC-035 só materializa LDMIA de ROM quando Rb é singleton. O helper runtime v3 atende RAM física e rejeita ROM. Após SPEC-036, a análise estática de LeafGreen observa `LDMIA r3!,{r0}` em `0x080006A8` com duas bases abstratas iniciais. Os bytes seguintes formam, por inferência, um loop com `CMP r1,#0; BGE`; a análise atual mantém ambos os sucessores e não converge a um conjunto finito útil.

## Decisão proposta

Preservar a imagem ROM como dado de compilação e introduzir uma side table IR de até 16 casos base→words, com LLVM emitindo despacho por base guest e constantes AOT. Singleton continua no caminho SPEC-035. O pacote recebe apenas as words lidas por cada caso; não recebe ROM inteira nem ponteiros de host.

Usar uma análise disjuntiva de estados completos desde a semente, por `(modo, PC)`, deduplicando apenas estados idênticos. Expansão de conjuntos finitos em registradores/memória produz alternativas singleton antes do enqueue; instruções ARM predicadas geram estados executado/não executado separados. Limites de recurso permitem até 64 alternativas completas por PC e 100.000 estados no worklist global; exceder qualquer budget rejeita sem merge. A sombra de memória mantém até 64 palavras por estado. Isso preserva relações como `{contador, base}` antes de alcançar o loop e não tenta reconstruí-las após um join lossy.

Refinar valores somente para o padrão Thumb adjacente `CMP imediato; B<cond>` e por alternativa. O fato de CMP identifica PC, registrador e imediato e só sobrevive ao branch imediatamente seguinte. Usar os flags aritméticos exatos de subtração para particionar alternativas conhecidas entre taken/fallthrough; alternativas sem fato correspondente ou com operando Unknown seguem ambos edges. A IR v2 registra a união dos sucessores provados após o fixed point. O lowering segue avaliando CPSR guest em runtime.

Uma base não incluída na tabela falha explicitamente em runtime com `EXIT_FAULT` e fault code aditivo 7. Se qualquer estado de um PC que tenha observado uma base ROM for Unknown ou não-ROM, se exceder 16 bases únicas, ou se sair dos bytes da imagem, a compilação falha após convergência. A detecção é sticky e independente da ordem do worklist. Strict-AOT nunca cai em helper de RAM, intérprete ou JIT para essas leituras.

## Alternativas

- Remover o guard de base singleton e deixar LDMIA cair no helper RAM: rejeitada porque ROM não pertence à ABI v3 e o pacote falharia de forma indireta ou poderia mascarar a região.
- Anexar o ROM inteiro ao executável e criar um helper de leitura dinâmica: rejeitada nesta tranche pelo custo de pacote, identidade/versionamento de imagem e expansão da ABI/runtime; avaliar quando evidência exigir endereços genuinamente desconhecidos.
- Refinar todo NZCV e todos os branches ARM/Thumb: adiado por ser maior e menos limitado. Esta decisão cobre apenas CMP imediato imediatamente seguido por branch Thumb.
- Unir casos ROM em uma constante só: rejeitada porque o valor lido depende da base runtime.
- Usar JIT/interpreter em base não mapeada: rejeitada para strict-AOT.

## Consequências

GBA IR passa da versão 1 à 2 por causa da side table e máscara de sucessores. ABI layouts e versions não mudam; somente o enum de fault ganha valor aditivo. Cada tabela é pequena, ordenada, validada por IR e limitada a 16 entradas. Branches cuja impossibilidade foi provada ainda contêm o teste de CPSR no binário; se um estado externo violar a especialização, o dispatcher produz saída missing-code/fault em vez de interpretar.

A implementação deve testar condições com overflow/carry, entrada alternativa sem o CMP, preservação de relações em loop, orçamento de alternativas por PC e orçamento global no limite e acima do limite, orçamento de casos ROM separado, fork de instruções ARM predicadas, loop sem limite, base desconhecida vista antes do primeiro caso ROM, table miss, atomicidade do estado guest, O0/O2, e regressão de LDMIA singleton/RAM. A tentativa de LeafGreen continua classificada como análise estática e não deve ser descrita como execução ou compatibilidade.

## Critérios para revisitar

- Um título exige mais de 16 bases comprovadas em um único LDMIA.
- ROM dinâmica que não possa ser limitada com o padrão CMP/branch exige avaliar uma ABI para imagem ROM de runtime.
- Benchmarks de pacotes reais justificam mudar o limite ou o desenho das tabelas.
- Oráculo REA/hardware apresenta divergência em LDMIA, flags de CMP ou branch.

## Referências

[SPEC-035](../specs/SPEC-035-thumb-ldmia.md), [SPEC-037](../specs/SPEC-037-thumb-ldmia-rom-cases.md), [SPEC-003](../specs/SPEC-003-discovery-overlays.md), [SPEC-004](../specs/SPEC-004-ir-llvm.md), [KNO-004](../knowledge/KNO-004-rom-intake.md), [KNO-005](../knowledge/KNO-005-isa-coverage.md).
