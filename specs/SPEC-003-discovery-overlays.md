# SPEC-003 — Descoberta, dispatch e código em RAM

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Código e dados podem coexistir na ROM. Destinos indiretos, cópias para RAM, overlays e código gerado impedem concluir cobertura por simples varredura.

## Requisitos normativos

- CFG-01: análise MUST combinar travessia recursiva da entrada com símbolos/hints validados, literais e jump tables com bounds demonstrados. Cada entrada tem origem `header|symbol|profile|static|trace` e confiança; dados desconhecidos não viram código por mera proximidade.
- CFG-02: identidade de execução MUST preservar endereço guest e estado ARM/Thumb. Mirrors podem compartilhar tradução somente se PC-relative, timing e acessos observáveis continuarem equivalentes.
- CFG-03: dispatcher MUST selecionar apenas funções nativas registradas em build. Endereço guest nunca é convertido diretamente em ponteiro de função host. Miss produz endereço/modo, ciclo, região e identidade esperada/observada.
- CFG-04: overlay MUST definir endereço de carga, extensão, hash do conteúdo e entradas. Origem pode ser cópia ROM, transformação declarada ou snapshot local identificado. Tradução MUST usar o endereço de execução.
- CFG-05: todas as escritas, inclusive DMA, BIOS HLE e restauração de estado, MUST invalidar páginas/versões de código afetadas. Guard MUST verificar integralmente a versão antes de entrar; write que afete instruções futuras do bloco exige sair ou revalidar antes do fetch afetado, respeitando o pipeline definido.
- CFG-06: `strict-aot` MUST parar em miss/mismatch e nunca interpretar. `aot-compatible` MAY interpretar mantendo estado/tempo e MUST contabilizar instruções, entradas e regiões interpretadas. Ambos MUST funcionar sem compilação em runtime.
- CFG-07: instrumentação MUST exportar hints por hash e snapshots de RAM apenas para armazenamento local solicitado. Uma sessão sem misses não comprova cobertura universal. Perfis não podem suprimir guards com um booleano sem prova verificável.
- CFG-08: descoberta MUST ter limites configuráveis de blocos/bytes/tempo e emitir `analysis_budget_exceeded` com trabalho parcial identificado, sem rotular análise incompleta como completa.

## Interfaces

`CodeKey { guest_pc:u32, isa, content_id }`; `DispatchResult = Translated | Missing | Invalidated | Fault`. Transferência direta otimizada mantém gate de scheduler/IRQ/invalidations. Hash criptográfico identifica overlay persistido; contador geracional pode acelerar validação local desde que não permita colisão/reuso stale.

## Aceite e matriz

ROM com literal pool não executado; jump table limitada; destino indireto ausente; duas versões no mesmo endereço; código copiado de ROM para IWRAM/EWRAM; write CPU e DMA no bloco ativo; mirror com PC-relative; restauração de snapshot após troca de overlay. Cada caso MUST chegar à versão correta ou a erro/fallback explícito correspondente ao modo.

## Observabilidade, limites e questões

Relatório separa cobertura estática candidata, execução observada e fallback por instrução; percentual por bloco não pode representar execução. Suporte a SMC visível no pipeline exige fixtures específicas. Fonte: KNO-001 SRC-03, KNO-002 e ADR-002.
