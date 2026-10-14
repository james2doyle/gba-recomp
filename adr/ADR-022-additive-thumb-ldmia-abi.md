# ADR-022 — Helper ABI aditivo para Thumb LDMIA

- Estado: Accepted
- Data: 2026-10-13
- Supersede: —

## Contexto

Thumb LDMIA precisa ler até oito words e gravar registradores em sequência. O runtime v3 expõe somente arrays host para EWRAM/IWRAM físicas; layouts ABI v2/v3/v4 não incluem o bus ou a ROM. Uma leitura guest não pode virar ponteiro host. LeafGreen bloqueia na instrução `0xCB01` em `0x080006A8`; discovery interno anterior ao decoder limita `r3` a `0x081E9F74`, e a ROM fixada nessa posição contém a word `0x08000821`. Isso é evidência de compilação/artefato estático, não execução do jogo.

## Decisão proposta

Adicionar o símbolo `gba_ldmia_thumb_v3(rt,out,base_register,register_list)`, sem alterar layouts nem números de versão ABI. O helper dinâmico aceita somente EWRAM/IWRAM físicas; valida todos os endereços primeiro, lê as words para temporários e só então atualiza os registradores e writeback. Alinha cada EA para baixo e preserva os bits baixos no writeback, como comportamento de LDMIA observado em mGBA e gba-recomp fixados e consistente com os sinais word do TRM ARM7TDMI. Falhas de região permanecem faults explícitos locais, não uma implementação de Data Abort.

Discovery pode produzir uma operação ROM materializada somente quando prova uma base singleton e cada word está na imagem imutável identificada pelo intake. A constante entra diretamente no código AOT; o pacote não recebe ponteiro guest/host para ROM nem copia o arquivo inteiro. Se a base dinâmica acessar ROM ou outra área sem suporte, o helper retorna fault; strict-AOT não interpreta nem usa JIT.

Lista vazia é rejeitada: o manual ARM a classifica como UNPREDICTABLE, embora os dois cores fixados deem a esse encoding um tratamento legado específico.

## Alternativas

- Alterar `GbaRuntimeV3`, `GbaRuntimeV4` ou `GbaMemoryV1` para incluir ROM/bus: rejeitada porque mudaria layout e não forneceria o modelo de mapeamento, efeitos ou sincronização necessários.
- Usar acesso direto do LLVM a um endereço guest: rejeitada porque guest address não é ponteiro host.
- Interpretar LDMIA não coberto: rejeitada para o modo strict-AOT.
- Exigir base alinhada: rejeitada para este recorte; os cores de referência alinham word EA e mantêm os bits baixos no writeback.

## Consequências

O helper compartilha as regras de atomicidade dos helpers de POP e mantém faults inspecionáveis via `GbaExitV3`. A região de RAM suportada e ROM singleton são limites deliberados, não bus geral. O modelo atual cobra um ciclo abstrato por instrução; timing e waitstates permanecem em SPEC-005. Nenhuma referência vendor será alterada ou copiada.

## Critérios para revisitar

- Implementação de bus geral em SPEC-005.
- Evidência de execução diferencial ou REA em LDMIA RAM/ROM.
- A ROM demonstrar base ROM multi-valued em um merge alcançável.

## Referências

[SPEC-035](../specs/SPEC-035-thumb-ldmia.md), [ADR-020](ADR-020-additive-thumb-pop-abi.md), [KNO-001 SRC-35](../knowledge/KNO-001-evidence.md), [KNO-004](../knowledge/KNO-004-rom-intake.md).
