# SPEC-012 — Primeiro homebrew recompilado

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Objetivo e limite deste incremento

GBA-020 deve percorrer uma ROM raw até código host executável usando uma fixture homebrew curta e determinística. O contrato cobre intake, descoberta de fluxo ARM direto, tradução AOT de um subconjunto declarado e execução headless com orçamento de instruções. Ele não declara compatibilidade com LeafGreen, BIOS, Thumb, periféricos, vídeo, áudio, input ou saves.

## Requisitos

- HOM-01: intake MUST ler a ROM como bytes imutáveis, preservar tamanho e calcular SHA-256 sobre o arquivo inteiro. O relatório MUST conter os campos de header e o resultado do checksum complementar; nome/caminho não participa da identidade.
- HOM-02: o ponto de entrada MUST ser `0x08000000`, em ARM. Leitura guest de código MUST ser limitada aos bytes disponíveis no cartucho; tamanho não potência de dois, endereço fora da imagem, alvo desalinhado e truncamento MUST produzir erro explícito.
- HOM-03: o decoder ARM inicial MUST aceitar somente `MOV` imediato sem flags, `ADD` imediato sem flags, `CMP` imediato e `B`/`BL` direto, cada qual com predicado ARM. Outras famílias, encodings reservados e modo Thumb MUST ser classificados como não implementados/indefinidos e impedir a emissão do pacote.
- HOM-04: descoberta MUST seguir o branch do entrypoint, fallthrough de instrução e ambos os sucessores de branch condicional. Branch incondicional encerra o fluxo sequencial; ciclos no CFG são permitidos. Todos os alvos alcançáveis MUST ser decodificados antes da publicação. O primeiro endereço duplicado com bytes/modo incompatíveis é erro.
- HOM-05: a IR MUST reter PC de origem, palavra ARM, modo, condição e operação. Verificação MUST rejeitar registradores inválidos, instruções não suportadas e fluxo sem sucessor conhecido. O backend MUST emitir um dispatcher nativo com um caso LLVM estático por PC descoberto; lookup sem caso retorna `missing_code` sem interpretar bytes guest.
- HOM-06: cada função MUST validar `abi_version` e overflow de `guest_cycle` antes de alterar CPU, CPSR, PC ou ciclo. O predicado ARM usa N/Z/C/V; `CMP` calcula N/Z/C/V com carry de ausência de borrow e overflow signed. A aritmética de registrador é módulo 2^32. `B` usa `PC+8+sign_extend(imm24)*4`; `BL` também escreve `LR=PC+4` quando sua condição passa. Condição falsa avança para `PC+4` sem outros efeitos.
- HOM-07: o executável host MUST chamar exclusivamente funções nativas geradas. Um limite explícito de instruções encerra a execução headless com estado final e motivo; PC sem função, ABI inválida ou falha guest retorna erro distinto. O manifest MUST registrar o hash ROM, LLVM/ABI/IR, otimização, total de instruções AOT e instruções executadas.
- HOM-08: esta fixture usa uma unidade abstrata por instrução em `guest_cycle`; isso serve apenas para exercitar commit/overflow, não é timing ARM7TDMI. O rótulo estrito-aot vale somente para instruções alcançáveis neste fixture e para a execução observada.
- HOM-09: falha de parse, descoberta, verificação, emissão ou link MUST deixar o caminho de saída sem um novo executável parcial. O pacote executável MUST ser independente de LLVM e não conter a ROM original.

## Fixture e critérios de aceite

`fixtures/tiny-counter` é uma ROM homebrew gerada de fontes textuais do projeto. Seu entrypoint salta para código ARM que inicializa `r0=0`, incrementa `r0` até 10 com `ADD`, `CMP` e `BNE`, e permanece em um branch próprio. Após exatamente 100 instruções, ambos O0 e O2 MUST reportar `r0=10`, `PC=0x080000D0`, CPSR com Z e C ligados, e 100 instruções executadas. O hash e os demais campos devem coincidir entre builds da mesma ROM.

| Caso | Resultado esperado |
|---|---|
| ROM fixture válida | Header e SHA-256 reproduzíveis; todos os alvos alcançáveis emitidos |
| O0 e O2 | Executáveis linkados, sem dependência dinâmica de LLVM, com estado final igual |
| Opcode alcançável fora do subconjunto | Falha antes de produzir executável |
| Branch desalinhado ou fora da ROM | Diagnóstico com PC e alvo; sem pacote parcial |
| ABI incompatível ou ciclo inicial `UINT64_MAX` | `fault` sem mutação de estado |
| ROM abaixo de 0xC0, acima de 32 MiB ou checksum inválido | Erro de intake; sem emissão |

## Questões em aberto

A promoção além desta fixture exige memória/bus e efeitos MMIO, ISA Thumb, BIOS/HLE, timing real, PPU/APU, input e backup conforme SPEC-001/002/005/006/007. Cobertura desta fixture não é uma medida de cobertura de uma biblioteca comercial. Evidência da ROM LeafGreen permanece em KNO-004 e sua questão REA não é resolvida por este contrato.
