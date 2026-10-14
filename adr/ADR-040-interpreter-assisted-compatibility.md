# ADR-040 — Modo AOT compatível com interpretação explícita

- Estado: Proposed
- Data: 2026-10-14

## Contexto

O contrato atual privilegia `strict-aot`: instruções alcançáveis sem lowering e destinos sem código resolvido bloqueiam o pacote. O vendor `gba-recomp` demonstra uma estratégia híbrida: instruções comuns/quentes são emitidas nativamente; instruções menos comuns ou que alteram modo/PC chamam `interp_one`, com sincronização de PC e retorno ao dispatcher quando o caminho diverge. A biblioteca LeafGreen examinada executou 60 frames e bateu com o interpretador do próprio vendor, mas teve misses e os contadores não incluem necessariamente cada shell interno. Isso é uma referência de arquitetura, não prova suficiente para adotar a implementação no produto.

O repositório local não contém um interpretador completo ARM7TDMI/GBA. `gba-ir::execute_instruction` e `execute_thumb_instruction` executam somente o subconjunto modelado pela IR e devolvem faults em operações que dependem de runtime ou memória; não podem servir como fallback geral. Além disso, bus, eventos e periféricos ainda estão incompletos.

## Decisão proposta

Manter `strict-aot` como padrão e especificar um segundo modo opt-in `aot-compatible`, sem JIT. Nesse modo, lowering nativo continua sendo preferido para operações frequentes aprovadas. Um interpretador local só atende instruções válidas cujo caso completo de CPU e hardware esteja implementado e verificado. Uso fica restrito a operações sem lowering que tenham perfil raro/custo alto, alvos dinâmicos, misses e guards de código mutável. Famílias candidatas da fonte vendor (shifts por registrador, PSR/bancos, efeitos de modo/PC/exception) permanecem exemplos a validar, não suporte afirmado.

O primeiro passo interpretado retorna ao dispatcher; continuação dentro do bloco é uma otimização permitida somente após verificação de PC sequencial, modo, eventos e validade do código. Misses nunca disparam JIT. Manifests, métricas e diferencial identificam todos os caminhos interpretados, inclusive shells gerados dentro de blocos.

Não integrar nem copiar o interpretador vendor nesta decisão. Uma reutilização futura exige análise de licença por arquivo, proveniência e adaptação à ABI local. mGBA/vendor podem atuar como referências diferenciais, respeitando suas limitações.

## Consequências

- A interpretação opt-in pode aumentar a cobertura inicial sem alegar que todo código virou nativo.
- Cenários compatíveis terão custo variável por passo interpretado; métricas por opcode/PC indicarão onde investir em lowering.
- A implementação depende de interpretador ARMv4T local, bus/runtime suficiente, versão de ABI/manifests e orçamento explícito.
- Instruções e efeitos não cobertos continuam falhando; semântica GBA não pode ser omitida sob o nome de compatibilidade.
- `strict-aot` permanece reprodutível e sem dependência do interpretador.

## Aceitação proposta

Aceitar somente após os critérios e fixtures da [SPEC-055](../specs/SPEC-055-interpreter-assisted-compatibility.md) passarem, incluindo equivalência diferencial de estado/eventos, contadores completos de passo interpretado, limites finitos e prova de que strict-AOT não muda.

## Referências

[SPEC-055](../specs/SPEC-055-interpreter-assisted-compatibility.md), [ADR-002](ADR-002-aot-and-profiles.md), [KNO-006](../knowledge/KNO-006-vendor-gba-recomp.md).
