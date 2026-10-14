# SPEC-049 — Preservar fatos de latches após DMA3 imediato ROM→IWRAM

Estado do documento: Draft. Especificação escrita antes da implementação. Progresso: [ROADMAP](../ROADMAP.md).

## Contexto e escopo

Depois da invalidação corrigida em SPEC-048, o trace interno da discovery mantém IE/IF/IME zero após Palette RAM e WAITCNT. O primeiro clear seguinte ocorre na rotina em `0x080006B4`: stores Thumb word configuram DMA3 em `0x040000D4`, `0x040000D8` e `0x040000DC`. O estado abstrato observado contém fonte `0x08000248`, destino `0x03003580` e word count/control `0x84000200`. Esses valores vêm do estado do compilador, não de uma execução da ROM. O trace e os limites estão em [evidência LeafGreen pós-WAITCNT](../knowledge/evidence/leafgreen-after-waitcnt-20261014-O2-state-trace.log).

Esta tranche acrescenta à descoberta estática um fato limitado sobre a configuração DMA3. Ela só pode preservar IE/IF/IME quando o estado inicial do runner é `strict-aot-runner-zero-v1`, todos os canais DMA permanecem comprovadamente inativos, e a sequência comprovada é uma transferência imediata de 32 bits da ROM carregada para IWRAM, com incremento de fonte/destino, uma execução e DoIRQ desabilitado. A regra não é suporte geral de DMA, não modela tempo/eventos, não copia bytes para a sombra abstrata e não muda ABI/runtime.

No mGBA fixado, habilitar DMA imediato agenda início após três ciclos e bloqueia a CPU durante a transferência. Esse detalhe real continua fora do modelo estático `interrupt_model: "none"`; o contrato assume que nenhuma fonte assíncrona atualiza IF enquanto a sequência é analisada. Uma mudança futura em eventos, IRQ ou DMA deve rever a hipótese antes de reutilizá-la.

## Requisitos normativos

- DMA-01: o fato abstrato DMA MUST iniciar com os quatro canais desabilitados e sem transferência pendente somente para a entrada `strict-aot-runner-zero-v1`, cuja máquina e arrays de I/O são zerados pelo runner. A falta de fato em outro estado de entrada MUST ser tratada como Unknown, nunca como canal ocioso.
- DMA-02: discovery MAY reconhecer apenas stores Thumb `STR` word imediato, com EA singleton, alinhado a palavra e exatamente em `DMA3SAD_LO` `0x040000D4`, `DMA3DAD_LO` `0x040000D8` ou `DMA3CNT_LO` `0x040000DC`. A sequência não depende de PCs ou encodings específicos da ROM. Qualquer store parcial, ARM store, EA alternativo, valor não singleton de control, ou acesso a DMA0–DMA2 MUST invalidar os fatos de I/O e o estado DMA relevante.
- DMA-03: stores em SAD/DAD MUST preservar IE/IF/IME apenas quando todos os canais são comprovadamente ociosos e o bit Enable de DMA3 é comprovadamente zero. O valor escrito pode ser guardado como fato abstrato para validar uma habilitação posterior; a store MUST sempre limpar `memory_words`.
- DMA-04: um store word em CNT MUST preservar IE/IF/IME sem iniciar transação se control é singleton e tem Enable zero; nesse caso discovery atualiza a prova de DMA3 desabilitado. Control Unknown ou com alternativas não singleton MUST invalidar os fatos de I/O.
- DMA-05: para uma escrita CNT que habilita DMA3, discovery MAY preservar IE/IF/IME somente se provar, antes da store, DMA3 desabilitado, demais canais ociosos, count singleton no intervalo `1..=0xFFFF`, fonte/destino singleton e word-alinhados, e control singleton que, após a máscara de bits implementada em mGBA para DMA3, seja `0x8400`. O padrão corresponde a enable, imediato, 32 bits, incremento em fonte e destino, sem repeat, DRQ ou DoIRQ. Outras combinações MUST limpar os fatos de I/O.
- DMA-06: antes de preservar latches após DMA imediato, discovery MUST verificar com soma checada que `[source, source + count*4)` está contido na imagem ROM carregada e `[destination, destination + count*4)` está contido em IWRAM física `[0x03000000,0x03008000)`. Overflow, endereço desalinhado, intervalo fora dessas regiões ou valor cujo mascaramento do registrador mude o alinhamento exigido MUST invalidar a prova e os fatos I/O.
- DMA-07: para a transação aceita em DMA-05/06, discovery MUST limpar toda a sombra `memory_words`; MUST NOT inferir conteúdo transferido, cargas de registradores DMA ou novos valores de memória. A conclusão estática pode registrar DMA3 como desabilitado após a transferência imediata de execução única, pois o CPU só prossegue depois do DMA completar. A regra não estima ciclos.
- DMA-08: acesso desconhecido/alternativo a registradores DMA, qualquer store aos controles de DMA0–DMA2, ou configuração DMA3 não reconhecida MUST tornar desconhecida a prova de quiescência. Nenhum store posterior poderá recuperar essa prova por convenção; somente reset de DMA garantido pelo HLE #1 com flag `0x80` singleton pode restaurar estado ocioso conhecido. Flags HLE que podem tanto resetar quanto preservar DMA MUST produzir Unknown.
- DMA-09: qualquer efeito reconhecido nesta spec afeta somente a discovery estática no perfil sem eventos. Código AOT ainda deve usar os helpers/ABI existentes; stores/loads MMIO DMA continuam faultando explicitamente no runtime atual. MUST NOT adicionar fallback interpretativo ou JIT, nem declarar o pacote LeafGreen executável.
- DMA-10: esta regra MUST ser revista quando o runner modelar fontes de IF, IRQ, timers, vídeo, áudio, DMA assíncrono ou ciclos de barramento. A fonte mGBA comprova o comportamento do modelo de referência fixado, não precisão em silício.

## Matriz de aceite

| Caso | Resultado verificável |
|---|---|
| Entrada all-zero com store Thumb word em SAD/DAD enquanto DMA3 desabilitado | Latches preservados, DMA3 atualizado e sombra RAM limpa |
| CNT word com enable zero singleton | Latches preservados e DMA3 continua conhecido como desabilitado |
| DMA3 imediato com control `0x8400`, count finito, ROM→IWRAM e sem DoIRQ | Latches preservados, DMA marcado concluído/desabilitado e sombra RAM limpa |
| DMA3 com IRQ, repeat, DRQ, timing não imediato, width 16-bit ou controle desconhecido | Latches e prova DMA invalidados |
| Fonte fora dos bytes da ROM carregada, destino fora de IWRAM, desalinhamento, overflow ou count zero | Latches e prova DMA invalidados |
| DMA0–DMA2 escrito antes da configuração de DMA3 | Latches invalidados; um DMA3 posterior não recupera quiescência |
| Store byte/halfword ou store ARM em registrador DMA | Caminho conservador existente; latches invalidados |
| HLE #1 com reset DMA singleton | Configuração DMA volta a todos canais ociosos |
| HLE #1 com flags que podem preservar ou resetar DMA | Prova de quiescência torna-se Unknown |
| Store DMA aceito ou rejeitado | Sombra `memory_words` fica vazia; conteúdo copiado não é inferido |
| Fixture sintética das três stores em O0/O2 | Discovery preserva latches apenas no perfil all-zero sem eventos; runtime/fault continua explícito |
| LeafGreen do SHA-256 de KNO-004 | Registrar se BX em `0x08000B6A` fecha ou qual próxima fronteira aparece; nenhum resultado de discovery será chamado de execução |

## Fontes e relações

- [SPEC-048 — invalidação de fatos I/O](SPEC-048-discovery-io-fact-invalidation.md).
- [SPEC-045 — latches IE/IF/IME](SPEC-045-thumb-ie-ime-latches.md), IOM-06/08.
- [SPEC-046 — estado inicial do runner](SPEC-046-strict-aot-entry-state.md), EIS-08.
- [SPEC-034 — memória abstrata](SPEC-034-abstract-memory-discovery.md), AMEM-04.
- [ADR-034 — DMA3 estático limitado](../adr/ADR-034-discovery-safe-dma3-latches.md).
- [SRC-43 — efeitos DMA3 de referência e limite local](../knowledge/KNO-001-evidence.md).
- [KNO-004 — tentativa estática LeafGreen](../knowledge/KNO-004-rom-intake.md).

mGBA no commit `143adf5aa2a60c6036810b0cd72445e13d74b0e6`: `include/mgba/internal/gba/io.h:109-114` nomeia os offsets DMA3; `include/mgba/internal/gba/dma.h:29-37` define os bits de controle; `src/gba/dma.c:21-33,81-145` mascara endereços/controle e agenda DMA imediato; `:199-229` limpa enable ao completar one-shot e eleva IRQ somente com DoIRQ; `:259-318` faz transferências de 32 bits. A referência inicia após três ciclos e bloqueia a CPU durante o serviço. Isso fundamenta somente os efeitos especificados no modelo estático sem eventos. O runtime local em `696b031f59f5f820adcaf8bfbf3cb6d0c2ef61ee` aceita stores word apenas em EWRAM/IWRAM (`crates/gba-runtime-abi/src/runtime_v3.c:35-71`) e continua faultando MMIO; não houve reutilização de código vendor.
