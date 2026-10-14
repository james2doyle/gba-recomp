# ADR-032 — Agendar fixtures Thumb STRB/LDRB em lote limitado

- Estado: Accepted
- Data: 2026-10-14
- Supersede: trecho de [ADR-021](ADR-021-bounded-gba020-gate-parallelism.md) que mantinha serial todo grupo ainda não listado

## Contexto

As cinco fixtures Thumb STRB e cinco LDRB são independentes: cada modo gera uma ROM, pacotes O0/O2 e saídas com nomes próprios. No gate atual, as dez são compiladas em série, embora os outros grupos independentes já usem o scheduler limitado de SPEC-043. Compilações paralelas precisam limitar LLD a uma thread por worker para que o produto permaneça dentro do orçamento configurado.

## Decisão

Adicionar um grupo `thumb-byte-load-store` com as cinco fixtures STRB e cinco LDRB, uma tarefa por modo. Aplicar o mesmo limite `GBA020_JOBS × GBA020_LINK_THREADS` da SPEC-043 e aguardar o grupo inteiro antes de iniciar o próximo. Sem overrides e com orçamento de dez CPUs, o padrão é dez workers × uma thread LLD. Com orçamento ou configuração menor, obedecer aos limites e defaults definidos por [SPEC-047](../specs/SPEC-047-logical-cpu-budget.md).

Cada worker compila O0 e O2, executa cada pacote, confere estado guest e manifesto, e escreve log exclusivo. Falha de qualquer worker faz o gate falhar depois de aguardar os workers do lote e imprimir seus logs. `GBA020_JOBS=1` preserva a execução serial. Nenhuma ROM comercial, fase de uma ROM, ou outro grupo é paralelizado por esta decisão.

## Alternativas

- Manter STRB/LDRB seriais: simples, mas deixa dez tarefas independentes fora do pool já existente.
- Paralelizar todas as fases ou grupos juntos: aumenta a competição de memória e dificulta a aplicação do teto de slots sem evidência de ganho adicional.
- Rodar dez workers sem limitar LLD: pode abrir até cem threads de link; não atende ao orçamento do gate.

## Consequências

O grupo conserva diretórios de ROM, pacote e log exclusivos por fixture. O padrão do host de referência acrescenta um lote de dez workers×uma thread LLD; a medição posterior deve registrar wall/user/system e não pode generalizar o resultado para a compilação de uma ROM comercial. Se o uso de memória ou a redução de tempo não forem adequados, o lote pode ser reduzido por `GBA020_JOBS`/`GBA020_LINK_THREADS` conforme SPEC-047.

## Referências

- [SPEC-043 — paralelismo limitado de fixtures](../specs/SPEC-043-bounded-fixture-parallelism.md).
- [SPEC-047 — orçamento de CPUs lógicas](../specs/SPEC-047-logical-cpu-budget.md).
- [ADR-021 — paralelismo do gate](ADR-021-bounded-gba020-gate-parallelism.md).
- [Gate GBA-020](../scripts/verify-gba020.sh).
