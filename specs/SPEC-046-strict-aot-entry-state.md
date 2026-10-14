# SPEC-046 — Estado inicial do runner strict-AOT e seed de discovery

Estado do documento: Draft. Implementação pendente. Progresso: [ROADMAP](../ROADMAP.md).

## Problema e escopo

O runner gerado inicializa `GbaRuntimeV4`, `GbaMachineStateV1` e `GbaExitV3` com zero, depois define CPSR para `0x0000001F` e PC para `0x08000000` antes de chamar o bloco compilado. Discovery, por sua vez, parte de registradores `Unknown` e só inicia IE/IF/IME em zero. Essa divergência faz a análise começar com menos fatos do que o runner fixa e ainda precisa de contrato explícito. A inspeção LLDB inicial de `AbstractValue` foi interpretada incorretamente. A trilha corrigida observou SP constante em B48 e explica a rejeição de B6A: no fallthrough, a STRH de IME em B58 não prova IF zero, invalida a sombra e faz o POP seguinte devolver r0 `Unknown`. Um probe posterior localizou a perda anterior de IF em `0x080003B8`, onde uma STRH para `0x05000000` limpa os fatos de I/O porque o endereço fica abaixo da janela MMIO. Isso descreve a política atual do analisador; não prova execução guest. A diferença entre seed e runner não foi provada como causa desse caminho.

Esta spec define somente o estado da entrada direta usada pelo runner headless strict-AOT atual, com o identificador `strict-aot-runner-zero-v1`, e exige que discovery use o mesmo estado como seed. Esse identificador descreve um contrato do produto independente da ROM. Não descreve power-on do GBA, estado após BIOS real, nem uma seed específica para LeafGreen.

A mudança de seed não garante, isoladamente, a resolução de PUSH/POP ou de um BX posterior. A transferência abstrata também precisa preservar os bancos de CPU através de MSR; a memória concreta zerada do runner continua distinta da sombra esparsa usada por discovery. Este contrato corrige a divergência verificável de entrada, sem atribuir a ela a fronteira LeafGreen ainda sob investigação.

## Estado de entrada do runner atual

`render_runner` em [`crates/gba-recomp/src/main.rs`](../crates/gba-recomp/src/main.rs) é a fonte executável deste contrato. O runner deve continuar materializando a seguinte entrada antes do primeiro bloco compilado:

| Campo | Valor inicial do runner |
|---|---|
| PC guest | `0x08000000` |
| Modo ISA | ARM; CPSR.T é zero |
| CPSR | `0x0000001F` (modo System, demais bits zero) |
| Registradores ativos | `r0–r14 = 0`; `r15 = 0x08000000` |
| Bancos de CPU | Todos os campos bancados presentes no ABI v2 são zero, inclusive `common_r8_r12`, `fiq_r8_r12`, `banked_r13_r14` e SPSRs |
| Ciclo guest | `0` |
| EWRAM e IWRAM | Todos os bytes zero |
| Demais arrays de `GbaMachineStateV1` | Palette RAM, VRAM, OAM, I/O e wave RAM começam zero |

`GbaMachineStateV1.abi_version`, versões ABI, ponteiros host e estrutura `GbaExitV3` são metadados do runtime, não estado guest: o runner os inicializa conforme os headers ABI depois do `memset`. A entrada começa diretamente na ROM; o runner não executa BIOS nem configura uma stack de boot. Portanto, SP guest (`r13`) é zero na entrada, e nenhum endereço convencional de stack deve ser presumido.

## Requisitos normativos

- EIS-01: o runner compilado MUST obedecer à tabela de estado acima antes de chamar `gba_block_v4`. Qualquer alteração semântica nesse estado MUST receber novo identificador de contrato; não pode reutilizar `strict-aot-runner-zero-v1` com valores diferentes.
- EIS-02: discovery para um pacote que declara `strict-aot-runner-zero-v1` MUST iniciar em `(ARM, 0x08000000)` com `r0–r14` como constantes zero, modo de privilégio `System`, ciclo zero e todos os valores bancados zerados. A abstração desta tranche não representa NZCV, I/F nem todos os bits de CPSR: branches sem `CompareFact` adjacente continuam mantendo ambos os sucessores conforme SPEC-037. Não pode inicializar registradores ou bancos como `Unknown` quando o runner os fixa.
- EIS-03: o estado inicial MUST vir de uma definição canônica compartilhada entre a geração do runner, discovery e manifesto, ou de definições equivalentes verificadas como iguais. A seed não pode ser um valor independente embutido apenas no analyzer.
- EIS-04: cada estado completo de discovery MUST carregar o valor abstrato do modo de privilégio CPSR, separado do modo ISA ARM/Thumb, e os valores de banco correlacionados ao mesmo caminho. Quando provados, modos alternativos MUST permanecer disjuntivos conforme [SPEC-038](SPEC-038-discovery-state-budget.md); não pode haver merge que associe um SP/LR a um modo diferente. Estados cujo modo seja `Unknown` MUST invalidar os valores de bancos que não puderem ser determinados. User e System compartilham banco, mas não privilégios. CPSR.T e o modo ISA continuam sujeitos a [SPEC-002](SPEC-002-armv4t-semantics.md) e [SPEC-018](SPEC-018-arm-thumb-branch-exchange.md); não são o campo M de privilégio.
- EIS-04a: discovery MUST reconhecer somente os modos M definidos em SPEC-013: User `0x10`, FIQ `0x11`, IRQ `0x12`, Supervisor `0x13`, Abort `0x17`, Undefined `0x1B` e System `0x1F`. Um `MSR` cujo modo atual seja comprovadamente inválido não tem sucessor executado, pois o runtime retorna fault; um modo de destino comprovadamente inválido também não gera sucessor bem-sucedido. Em instrução condicional, o caminho não executado continua sendo analisado. Modo de origem ou valor de origem não comprovado MUST produzir alternativas válidas explicitamente limitadas ou marcar modo e bancos afetados `Unknown`; nunca pode ser normalizado silenciosamente.
- EIS-05: discovery MUST modelar bancos `r8–r14` e transições de modo de `MSR CPSR` já suportadas, aplicando as máscaras e permissões de [SPEC-013](SPEC-013-banked-cpu-and-psr.md). `MSR` somente com campo `f` MUST preservar modo e bancos; escrita de campo `c` em modo User MUST preservar modo e banco User/System. Para transição privilegiada provada, MUST salvar registradores ativos no banco de origem e carregar o banco de destino, com a troca FIQ/common para `r8–r12` e bancos de `r13–r14`. Para modo desconhecido ou `MSR` com operando desconhecido, discovery MUST invalidar os fatos potencialmente afetados, não copiar valores entre bancos. NZCV permanece conservador segundo EIS-02; este requisito não autoriza eliminar caminhos condicionais sem prova independente.
- EIS-06: PUSH/POP e BX MUST consumir os valores abstratos de SP, memória, registradores e bancos da entrada/transferências anteriores. A seed dá `r13=0` no banco User/System; isso não é um SP de boot, endereço de stack presumido nem garantia de que um PUSH subsequente caiba em EWRAM/IWRAM. SP `Unknown`, desalinhado ou fora das regiões físicas aceitas não pode criar fatos de stack; slots ausentes permanecem `Unknown`, e BX sem destino finito continua sendo erro strict-AOT conforme SPEC-018/SPEC-034. A seed não autoriza relaxar bounds nem aumentar budgets. A evidência LLDB corrigida registra dois estados em B50: o branch direto recupera LR=`0x08000B95` pelo POP e fecha o BX em B6A; o fallthrough perde a sombra na STRH de IME em B58, pois IF não está provado zero, e mantém r0 `Unknown` após o POP em B68. Uma inspeção posterior localizou a perda anterior de IF em B3B8, numa escrita para `0x05000000` que limpa os fatos de I/O do analisador. Isso é estado de discovery, não execução da ROM. Consulte [o registro inicial supersedido](../knowledge/evidence/leafgreen-discovery-stack-state-lldb-initial-superseded.json) e [a correção](../knowledge/evidence/leafgreen-bx-r0-audit-20261014.json).
- EIS-07: o estado concreto de EWRAM/IWRAM e de todas as arrays de `GbaMachineStateV1` na entrada é zero. Este fato descreve o runner; nesta tranche, não obriga a sombra abstrata de discovery a representar memória zerada. Discovery MUST inicializar em zero os latches que já modela, incluindo IE/IF/IME conforme [SPEC-045](SPEC-045-thumb-ie-ime-latches.md). Para RAM ou I/O que seu domínio ainda não representa, discovery MAY manter `Unknown` como sobreaproximação; MUST NOT converter essa ausência em zero nem usar tal valor para fechar BX. Propagar zeros concretos de RAM para loads abstratos requer extensão separada da abstração AMEM que represente e invalide corretamente todos os efeitos de escrita possíveis.
- EIS-08: manifesto strict-AOT MUST registrar `entry_state.id = "strict-aot-runner-zero-v1"`, CPSR, PC, modo ISA, ciclo e políticas de inicialização de registradores/bancos e máquina conforme a tabela. O campo `machine_initialization` descreve somente `GbaMachineStateV1`; CPU e `GbaExitV3` são descritos separadamente. `io_initialization` MUST continuar como `"all-zero"`. O manifesto MUST versionar o schema ao tornar `entry_state` obrigatório; o identificador não pode ficar implícito no executável. Se um cache por conteúdo for introduzido, sua chave MUST incluir o identificador do estado.
- EIS-09: o identificador desta spec é uma política fixa do runner atual, não um profile por jogo nem um override de `SP`. Um estado de handoff alternativo MUST exigir contrato futuro próprio, vinculado ao SHA-256 exato conforme SPEC-008 e com evidência para CPU, bancos, memória, I/O e BIOS/HLE relevantes. Esse profile futuro não pode ser inferido nem habilitado por esta spec.
- EIS-10: `strict-aot-runner-zero-v1` MUST ser descrito como entrada direta sintética do runner. A aceitação desta spec não declara boot real, compatibilidade de hardware, resolução de BIOS, nem compilação ou execução de LeafGreen. Não pode haver fallback para interpretação ou JIT.

## Manifesto

O manifesto MUST expor um objeto versionado equivalente a:

```json
{
  "format_version": 2,
  "entry_state": {
    "id": "strict-aot-runner-zero-v1",
    "isa_mode": "arm",
    "pc": "0x08000000",
    "cpsr": "0x0000001F",
    "registers_r0_r14": "all-zero",
    "banked_cpu_state": "all-zero",
    "guest_cycle": 0,
    "machine_initialization": "zeroed-then-abi-version"
  },
  "interrupt_model": "none",
  "io_initialization": "all-zero"
}
```

`format_version` MUST avançar de 1 para 2 quando `entry_state` se tornar obrigatório, conforme IMG-05. Essa declaração não substitui hash da ROM, versões ABI/IR, política BIOS ou os demais campos exigidos por SPEC-001. O objeto identifica o runner; não afirma que o estado inicial é produzido por hardware ou BIOS.

`banked_cpu_state: "all-zero"` descreve o armazenamento ABI presente no runner. SPSRs zerados são apenas campos de armazenamento: User/System não têm SPSR arquitetural conforme BANK-02. O estado do manifesto descreve o estado concreto; discovery mantém NZCV conservador e não usa flags iniciais para remover caminhos.

## Fora de escopo e limites

- Reset/power-on do GBA, inicialização da BIOS e handoff de BIOS para ROM.
- Escolha de um SP real de boot, seed LeafGreen ou valores obtidos por convenção/trace sem contrato e evidência.
- Alteração de layout ABI, modelo geral de memória, eventos, IRQ, DMA, timing ou interpretação dos estados `Unknown`.
- Profiles de inicialização alternativos. A schema atual de SPEC-008 não contém estado inicial de CPU; sua extensão requer spec separada, validação de hash e digests.
- Propagação geral dos bytes zero de EWRAM/IWRAM para a sombra abstrata. O runner concretamente zera essas regiões, mas a sombra permanece conservadora: ausência de fato significa `Unknown` conforme SPEC-034. Uma extensão que use esses zeros para fechar destinos exige contrato próprio para cargas, invalidação por stores e efeitos não modelados.

A RAM zero descrita aqui é a inicialização concreta do runner, como delimitado por [SPEC-016/STR-06](SPEC-016-ram-word-stores.md). Ela não prova RAM zero em power-on real. O domínio atual da sombra é esparso e ausências significam `Unknown` segundo SPEC-034; essa semântica permanece nesta tranche. A seed CPU e a abstração mode-aware dos bancos são requisitos desta spec, mas não autorizam usar zeros concretos de RAM como fatos abstratos.

## Critérios de aceite

| Caso | Resultado verificável |
|---|---|
| Inicialização do runner | Antes do primeiro bloco: CPSR `0x1F`, PC `0x08000000`, r0–r14 zero, guest cycle zero, bancos e memória conforme o contrato |
| Seed de discovery | Estado inicial abstrato coincide com o runner; r13 começa como zero, não `Unknown` nem endereço de stack presumido |
| EWRAM/IWRAM e arrays de máquina | Runner observa bytes iniciais zero; discovery usa zero somente onde sua abstração mantém essa prova |
| IE/IF/IME | Discovery começa com os três latches em zero conforme SPEC-045; I/O não modelado não vira constante por inferência |
| MSR sem troca de modo ou somente campo `f` | Fatos bancados e modo de privilégio permanecem intactos; discovery conserva ambos os sucessores de branch quando não há prova de flags |
| MSR com troca provada | SP/LR e bancos FIQ/common correspondem ao modo de origem/destino segundo SPEC-013, mantendo modo e bancos correlacionados por estado |
| MSR em User | Escrita de controle não altera modo nem seleciona outro banco, conforme PSR-03 |
| MSR com modo desconhecido/múltiplo | Alternativas exatas limitadas pelos budgets, ou modo/bancos afetados `Unknown`; não há cópia entre bancos |
| Modo atual/destino reservado | Caminho executado sem sucessor bem-sucedido; em MSR condicional o caminho não executado permanece; sem normalização a outro modo |
| `PUSH` com SP zero | Discovery não registra palavra de stack fora de EWRAM/IWRAM; runtime faulta atomicamente pelo contrato TPUSH-04; não se infere progresso do jogo |
| PUSH/POP com SP provado | Discovery usa o banco ativo e os fatos de stack correspondentes |
| PUSH/POP com SP igual a zero, desconhecido ou fora do subset | Zero é propagado como valor de entrada, mas não cria stack fora das regiões aceitas; nenhum fato de stack inventado e BX dependente continua rejeitado sem pacote parcial |
| `BX` após retorno de stack | Fecha somente se todos os estados predecessores fornecem conjunto finito de destinos válidos pela SPEC-018 |
| Manifesto e identidade | `entry_state` descreve valores efetivamente usados pelo runner e seed da discovery; mudança de contrato invalida pacotes; cache futuro inclui `entry_state.id` |
| Perfil por jogo | Nenhuma seed LeafGreen é aceita por esta spec; profile futuro exige contrato/schema e evidência próprios |
| O0/O2 strict-AOT | Fixtures exercitam os casos de estado inicial e bancos sem fallback; falhas não publicam pacote parcial |

## Fontes e relações

- [SPEC-001 — intake e manifesto](SPEC-001-image-intake.md), IMG-04/05.
- [SPEC-002 — CPU ARMv4T](SPEC-002-armv4t-semantics.md), CPU-02/03/13.
- [SPEC-008 — profiles](SPEC-008-game-profiles.md), PROF-01–06.
- [SPEC-013 — bancos e MSR](SPEC-013-banked-cpu-and-psr.md), BANK-01/02 e PSR-01–05.
- [SPEC-016 — runner e RAM inicial](SPEC-016-ram-word-stores.md), STR-06.
- [SPEC-018 — BX strict-AOT](SPEC-018-arm-thumb-branch-exchange.md), BX-05/06.
- [SPEC-034 — sombra de discovery](SPEC-034-abstract-memory-discovery.md), AMEM-01–08.
- [SPEC-038 — budgets disjuntivos](SPEC-038-discovery-state-budget.md), DSB-01/02/06.
- [SPEC-045 — latches I/O](SPEC-045-thumb-ie-ime-latches.md), IOM-06/08.
