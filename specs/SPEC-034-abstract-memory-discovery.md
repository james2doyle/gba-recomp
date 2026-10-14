# SPEC-034 — Memória abstrata limitada para descoberta AOT

Estado do documento: Accepted para AMEM-01–09. Implementação original strict-AOT concluída no commit `d74148b`; AMEM-09 será implementado com a [SPEC-036](SPEC-036-thumb-stmia.md). Evidência original em [gba020-abstract-memory-macos-arm64.json](../knowledge/evidence/gba020-abstract-memory-macos-arm64.json); progresso geral no [ROADMAP](../ROADMAP.md).

## Contexto e escopo

SPEC-018 exige que BX feche todos os destinos possíveis com valores guest comprovados. O analyzer atual acompanha registradores, mas não valores escritos por Thumb PUSH e lidos por Thumb POP; por isso perde valores de LR salvos em frames conhecidos. Esta spec acrescenta uma sombra esparsa e limitada de palavras escritas por PUSH Thumb.

A sombra serve somente para descoberta estática. Ela não inicializa SP ou RAM, não altera IR, runtime, ABI, semântica guest, ciclos, ou política de strict-AOT. Não modela memória geral nem cobre leituras diferentes de POP Thumb, mirrors, MMIO ou conteúdos de RAM anteriores ao primeiro PUSH rastreado.

## Requisitos normativos

- AMEM-01: cada estado abstrato MAY conter até 64 fatos de palavra guest no formato `endereço alinhado -> AbstractValue`. Um endereço ausente significa `Unknown`; a entrada da ROM começa sem fatos de memória. Valores mantêm o limite de 16 constantes já usado por BX. Exceder o limite de palavras descarta todos os fatos de memória do estado, mantendo a descoberta conservadora e limitada.
- AMEM-02: Thumb PUSH MUST capturar os registradores selecionados do estado de entrada, na ordem r0–r7 e LR por último. Só pode registrar fatos quando SP contém exatamente um valor alinhado e todo o intervalo da operação cabe em EWRAM/IWRAM físicas. Antes de registrar, deve invalidar fatos sobrepostos pelos bytes escritos. Slots com valor desconhecido permanecem ausentes. Com SP desconhecido, múltiplo, desalinhado ou intervalo fora das regiões aceitas, a análise limpa os fatos de memória. A propagação de SP continua conforme TPUSH-05.
- AMEM-03: Thumb POP MUST consultar cada palavra em SP, em ordem crescente para os registradores selecionados. Cada destino recebe o valor finito rastreado naquele endereço ou `Unknown` quando o slot não existir. SP continua sendo propagado conforme TPOP-07. POP remove da sombra todos os endereços que podem ter sido consumidos.
- AMEM-04: Thumb STRB/STRH e ARM STR word MUST invalidar qualquer fato cuja palavra de quatro bytes sobreponha bytes que a escrita pode alterar. Uma escrita halfword considera o endereço efetivo alinhado para baixo conforme o helper runtime. Se o endereço efetivo for desconhecido, não enumerável, sofrer overflow durante a análise, ou não puder ser limitado às regiões físicas aceitas, a análise limpa todos os fatos de memória. Stores não criam fatos nesta tranche.
- AMEM-05: um merge MUST manter somente endereços presentes em todos os estados predecessores e unir os valores desses endereços pelo domínio existente de constantes finitas. Endereço ausente em um predecessor equivale a `Unknown`; união acima do limite de constantes remove aquele fato. Stores condicionais usam o merge entre caminho executado e não executado.
- AMEM-06: SWI #1, continuação de ARM BL sintetizada sem análise dos efeitos do callee, e qualquer efeito de RAM não representado MUST limpar todos os fatos de memória. Chamadas Thumb BL para código ROM analisado podem propagar fatos; cada store conhecido é invalidado conforme AMEM-04, e SWI #1 limpa os fatos. Alterações de registradores guest não podem assumir convenção de ABI host.
- AMEM-07: um destino BX só pode ser fechado com a abstração resultante desta spec quando todo caminho analisado fornece valor finito para o operando. Qualquer caminho com slot ausente continua gerando `bx_target_unresolved`; strict-AOT não pode cair em interpretação ou JIT.
- AMEM-08: a análise MUST convergir dentro dos limites existentes de instruções e atualizações e do limite AMEM-01. A sombra não pode usar scripts ou dados executáveis recebidos em perfis.
- AMEM-09: a extensão da SPEC-036 MUST permitir Thumb STMIA T1 atualizar a sombra somente com base singleton e intervalo completo em uma região física suportada. Deve usar os registradores de entrada antes de writeback; palavras com valor Unknown são removidas. Base desconhecida/múltipla, wrap ou intervalo fora do subset limpa todos os fatos de memória.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| PUSH de constantes seguido de POP | Registradores recebem valores na ordem definida; SP pode continuar desconhecido sem inventar um valor |
| PUSH com LR | POP recupera LR no slot final, após r0–r7 selecionados |
| PUSH com registrador desconhecido | POP correspondente permanece desconhecido |
| POP sem fato de memória | Destino permanece desconhecido e BX correspondente é rejeitado |
| STRB/STRH/STR word sobre um slot rastreado | Somente o fato sobreposto é removido; slots não sobrepostos permanecem |
| Store com endereço desconhecido ou não limitado | Todos os fatos de memória são removidos |
| Merge de dois caminhos com mesmo/diferente valor no slot | Fato preservado com constante/conjunto finito correspondente |
| Merge em que um caminho não tem o slot | Fato removido; POP resulta Unknown |
| SWI #1 ou continuação conservadora de ARM BL | Todos os fatos de memória são removidos |
| Mais de 64 fatos ou mais de 16 valores em um slot | Descarta fatos conforme AMEM-01/05 e converge |
| LeafGreen no SHA-256 fixado | Descoberta fecha o retorno em `0x080009BE` somente se todos os predecessores analisados fornecerem destino finito; a nova fronteira é registrada sem alegar execução da ROM |

## Verificação necessária

Cobrir unitariamente transferência, ordem, slots ausentes, limites, overflow, escritas parciais, união de caminhos e invalidação conservadora. Incluir fixture ROM que salva um endereço Thumb, o remove de um registrador, restaura-o por POP e fecha BX; fixture negativa com caminho sem valor; e fixture com `STRB [SP]` que invalida o slot superior sem apagar um LR salvo em endereço distinto. O gate deve executar fixtures strict-AOT O0/O2 e verificar falha sem pacote para o destino desconhecido.

Registrar compilação estática da ROM fixada e sua fronteira. Compilação da ROM não prova execução do jogo nem cobertura total de seus caminhos. Não afirmar decompilação sem Evidence ID do REA.

## Relação com contratos existentes

Esta extensão refina somente os efeitos de descoberta de [SPEC-019](SPEC-019-thumb-push.md) e [SPEC-033](SPEC-033-thumb-pop.md). A validade dos destinos segue [SPEC-018](SPEC-018-arm-thumb-branch-exchange.md); chamadas Thumb e SWI seguem [SPEC-022](SPEC-022-thumb-bl-pair.md) e [SPEC-023](SPEC-023-thumb-swi-register-ram-reset.md). Fontes fixadas em SRC-20/SRC-34 corroboram semântica de transferência, sem provar valores da ROM. A ABI guest e os contratos de memória runtime não mudam.
