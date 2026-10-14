# SPEC-055 — Interpretação explícita em pacotes AOT compatíveis

Estado do documento: Draft. Progresso e gates de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

O modo `strict-aot` exige lowering nativo para todo caminho guest alcançável que o pacote promete cobrir. Essa política é previsível, mas bloqueia a geração quando uma instrução válida tem semântica conhecida e ainda não tem lowering LLVM. Um modo opcional pode combinar blocos nativos com um interpretador ARMv4T do próprio produto, desde que cada entrada interpretada seja explícita, observável e coberta por contratos de CPU e hardware.

Esta especificação define a seleção e o contrato de um futuro modo `aot-compatible`. Não implementa um interpretador completo, não integra o interpretador de `vendor-ref/gba-recomp`, não adiciona JIT e não relaxa o modo `strict-aot`.

O `execute_instruction`/`execute_thumb_instruction` existente em `gba-ir` executa somente as operações representadas pela IR atual e retorna faults quando precisa de runtime bancado, memória ou literais não resolvidos. Não é um decoder nem um interpretador completo de ARM7TDMI e não pode ser usado como fallback geral. O modo compatível depende de uma implementação local que satisfaça este contrato e os contratos de CPU, bus, eventos, BIOS e memória aplicáveis ao cenário declarado.

## Vocabulário

- **Lowering nativo**: instrução guest emitida como LLVM/nativo no bloco compilado.
- **Shell de interpretação**: ponto de chamada que sincroniza o estado guest, executa um passo guest pelo interpretador e devolve controle ao bloco ou dispatcher conforme o próximo estado.
- **Miss de bloco**: PC/modo/identidade de código sem entrada compilada no dispatcher.
- **Passo interpretado**: uma instrução guest aposentada pelo interpretador; chamadas internas e misses não podem ser confundidos com instruções aposentadas.

## Requisitos normativos

- IAC-01: `strict-aot` MUST continuar sendo o modo padrão. O compilador e o pacote desse modo MUST rejeitar instruções alcançáveis sem lowering e destinos sem código resolvido segundo as specs existentes. MUST NOT chamar interpretador ou JIT em resposta a uma instrução, miss, guard ou falha de tabela.
- IAC-02: o usuário MUST selecionar `aot-compatible` explicitamente pela CLI ou por configuração técnica explicitamente ativada. A seleção MUST aparecer no diagnóstico de compilação, no manifesto do pacote e no estado do runner. Uma falha de cobertura nunca pode trocar o modo silenciosamente.
- IAC-03: `aot-compatible` MUST emitir lowering nativo sempre que existir lowering aceito para a instrução e seus efeitos. Uma instrução sem lowering só pode ser enviada ao interpretador se o decoder a classificar como instrução ARMv4T definida, o interpretador local declarar suporte exato para o caso e todos os efeitos de memória, modo, evento e exceção necessários estiverem implementados. O interpretador MUST NOT converter opcode indefinido, extensão fora de ARMv4T ou semântica ausente em NOP/sucesso.
- IAC-04: a política de seleção SHOULD priorizar interpretação para operações medidas como raras ou de alto custo de lowering, e para passos cujo destino guest não pode ser fechado estaticamente. O perfil de frequência MUST vir de trace ou execução identificada por hash; nomes de opcode ou expectativa de baixa frequência não contam como medição. ALU, branches e acessos de memória frequentes com semântica e lowering aprovados devem permanecer nativos. Famílias iniciais candidatas a avaliação incluem shifts especificados por registrador, transferências de PSR/bancos, operações que alteram modo/PC e instruções de controle de exceção; cada uma requer semântica local validada antes de ser habilitada.
- IAC-05: a unidade mínima de fallback MUST ser um passo guest. Antes do passo, o runner MUST materializar PC, modo, CPSR, bancos e estado de máquina segundo a ABI. O interpretador MUST atualizar o estado guest e devolver resultado estruturado: próximo PC/modo, ciclos, estado de evento/interrupção, fault e quantidade de instruções aposentadas. Endereços guest permanecem inteiros e acessos de memória passam pelo bus/ABI GBA; ponteiros guest nunca são ponteiros host.
- IAC-06: um shell dentro de bloco MUST descarregar ciclos acumulados e sincronizar o PC guest antes de chamar o interpretador. Pode continuar na sequência nativa somente quando o interpretador aposentou exatamente uma instrução, retornou o PC sequencial previsto no mesmo modo, não deixou evento que exija despacho e o código compilado continua válido. Branch, alteração de modo/PC, IRQ/evento, fault, invalidação ou divergência MUST retornar ao dispatcher. A implementação inicial MAY retornar ao dispatcher após todo passo interpretado.
- IAC-07: em miss de dispatcher, `aot-compatible` MAY interpretar um passo e repetir lookup pelo novo estado; MUST NOT compilar código durante a execução. Os limites de instruções interpretadas e de passos por frame MUST ser configuráveis com defaults finitos. Ao exceder limites, reportar fault/stop explícito preservando estado committed.
- IAC-08: código em RAM ou outra região mutável MUST ser guardado pela identidade do conteúdo executável e estado ISA. Falha de guard pode executar pelo interpretador somente no modo compatível; writes que invalidem código compilado MUST invalidar ou versionar o bloco antes de qualquer execução stale. Em `strict-aot`, guard/miss mantém o fault previsto pelos contratos atuais.
- IAC-09: o manifesto compatível MUST identificar `execution_mode`, versão/hash do interpretador, versões de CPU e ABI/runtime, ROM SHA-256, mapa de suporte interpretado, limites de fallback e dependências do pacote. O runner compatível MUST incluir o interpretador/runtime necessário; o runner strict-AOT continua sem essa dependência.
- IAC-10: métricas MUST contar separadamente instruções nativas aposentadas, instruções interpretadas aposentadas por família e PC, misses do dispatcher, shells interpretados, guards/invalidações, ciclos interpretados e faults. Cada passo guest interpretado deve contribuir para exatamente uma contagem de instrução interpretada; miss e chamada de shell não substituem essa contagem. O relatório MUST declarar denominador e janela da taxa de interpretação.
- IAC-11: execução diferencial MUST comparar o mesmo estado inicial e entradas entre caminho inteiramente interpretado e pacote compatível, por PC, modo, registradores, CPSR, ciclos, memória visível e eventos/saída determinística disponíveis. A comparação com o próprio interpretador é necessária para testar o híbrido, mas MUST ser acompanhada de testes semânticos independentes e de uma referência externa adequada; não prova compatibilidade geral do jogo nem correção de hardware.
- IAC-12: ativar `aot-compatible` MUST NOT alterar semântica guest, estado inicial, comportamento de preferências ou identidade de cache sem registro explícito. Chaves de cache MUST incluir modo de execução, hash/versão do interpretador, ABI/runtime e limites que alterem artefatos.

## Critérios para decidir onde interpretar

| Situação | Política contratada |
|---|---|
| Instrução frequente com lowering e semântica validados | Emitir nativo; medir para evitar regressão de desempenho |
| Instrução válida sem lowering, rara ou cara de baixar, com suporte completo no interpretador local | Pode usar shell em `aot-compatible`; registrar frequência e custo |
| Branch/BX ou código guest cujo destino não foi fechado estaticamente | Interpretar um passo e despachar novamente, se CPU/bus/eventos suportarem o caso |
| Código mutável sem snapshot atual ou bloco com guard inválido | Interpretar a partir do PC corrente ou fault explícito conforme o modo; nunca executar bloco stale |
| Opcode indefinido, fora de ARMv4T ou sem semântica no interpretador | Rejeitar/faultar com diagnóstico; não usar como escape para cobertura incompleta |
| Instrução com acesso MMIO, SWI, IRQ ou efeito de bus sem modelo local verificado | Não elegível até seu contrato de hardware ser implementado e testado |

Uma família não é elegível só por ser rara ou difícil de emitir. Uma família não deve permanecer interpretada por conveniência se o trace mostrar que ela domina a execução; registrar o perfil e avaliar lowering nativo.

## Interface e manifesto propostos

O modo de execução é um campo versionado do manifesto, com valores `strict-aot` e `aot-compatible`. Um pacote compatível declara explicitamente o dispatcher de misses e os shells habilitados; o modo não pode ser deduzido pela presença de uma biblioteca do interpretador.

O resultado de um passo interpretado precisa representar pelo menos `continue`, `event`, `halt`, `fault` e `stop`, além de `next_pc`, modo, ciclos committed e contagem de instruções. Falha do interpretador é terminal para aquele passo e não pode cair em JIT ou ignorar o efeito.

O schema exato de ABI/manifesto e o conjunto de funções de runtime serão definidos numa spec de implementação após CPU e bus poderem preservar atomicidade e observabilidade no passo interpretado. Esta spec não autoriza reutilizar ou alterar `vendor-ref/`.

## Matriz mínima de aceitação da implementação futura

| Caso | Resultado esperado |
|---|---|
| Mesmo cenário `strict-aot` antes/depois | Continua sem chamada ao interpretador; lacuna de lowering falha sem pacote parcial |
| Instrução elegível em `aot-compatible` | Executa um passo interpretado e atualiza estado/ciclo/eventos de acordo com contrato CPU/bus |
| Instrução com lowering existente | Continua pelo bloco nativo e é contada como nativa |
| Branch dinâmico ou miss de bloco | Interpreta um passo, refaz lookup; nenhum código novo é emitido |
| Passo interpretado com PC sequencial | Pode continuar no bloco só com os guards de IAC-06; caso contrário volta ao dispatcher |
| Troca de modo, evento pendente, fault ou PC divergente | Retorna ao dispatcher/fault sem executar a instrução seguinte no modo errado |
| Opcode indefinido ou caso não implementado | Diagnóstico/fault explícito sem NOP nem fallback permissivo |
| Escrita concorrente/auto-modificação de código guest | Bloco antigo é invalidado antes do próximo uso |
| Métricas | Shells, misses e passos aposentados são contados separadamente, inclusive passos em shells internos |
| Diferencial | Pacote compatível coincide com execução interpretada nas dimensões e janela contratadas em O0/O2 |

A aceitação também exige fixtures de limites de ciclo, rollback/atomicidade de fault, interworking ARM↔Thumb, acesso físico e MMIO suportados, estados bancados, SWI/eventos cobertos, invalidação de RAM e limites de orçamento. Cada cenário qualificado é identificado pela ROM SHA-256, modo, hash de entradas e duração/traces cobertos; zero fallback numa amostra curta não prova `full recomp` nem compatibilidade do título.

## Proveniência e limites da referência

O desenho usa como evidência de arquitetura o comentário e os caminhos de `vendor-ref/gba-recomp/crates/recomp-core/src/emit.rs:3-8,124-135,547-570`: o vendor inlineia classes quentes, usa shells para classes raras/de mudança de modo e volta ao dispatcher quando o PC não segue a sequência prevista ou um guard falha. Seu dispatcher interpreta misses quando habilitado. Essa observação orienta a política, não prova semântica local; a biblioteca LeafGreen e o diferencial de 60 frames têm os limites documentados em [KNO-006](../knowledge/KNO-006-vendor-gba-recomp.md). Nenhum código vendor é copiado por esta spec.

## Referências

[ADR-040](../adr/ADR-040-interpreter-assisted-compatibility.md), [SPEC-002](SPEC-002-armv4t-semantics.md), [SPEC-003](SPEC-003-discovery-overlays.md), [SPEC-004](SPEC-004-ir-llvm.md), [SPEC-005](SPEC-005-runtime-timing.md), [KNO-005](../knowledge/KNO-005-isa-coverage.md), [KNO-006](../knowledge/KNO-006-vendor-gba-recomp.md).
