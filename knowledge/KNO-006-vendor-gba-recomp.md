# KNO-006 — Avaliação do vendor gba-recomp

Data: 2026-10-14. Fonte: checkout local `vendor-ref/gba-recomp`, commit `de4edf59b872d887046d6a3b005e2df551b6d44c`. O checkout vendor permaneceu inalterado. A avaliação inclui leitura de código, documentação e metadados SQLite, build da ROM específica e comparação de execução em macOS arm64.

## Conclusão

O vendor cobre a fronteira atual `0x428A` por fonte: decodifica e emite Thumb `CMP r2,r1`. Além disso, o comando genérico `recomp build` compilou a LeafGreen fixada e gerou uma biblioteca compartilhada arm64 de 6.101.744 bytes. A biblioteca executou 60 frames; o hash de framebuffer bateu com o interpretador do próprio vendor. Nesse trecho, `runc` contou 4.090 blocos nativos e 35.586 passos de fallback por falta de bloco. O contador não inclui chamadas ao interpretador feitas por shells `interp_one` dentro de blocos, então não mede todo o trabalho interpretado.

O `gamedb.sqlite` disponível no checkout tem 1.281 funções para um único SHA-256 (`2dac7cdf9821c7ef672b5700f0114cf8bbe46bd019f85b97478e4c9a937637c5`), diferente da LeafGreen V1.1 fixada aqui (`2f978f635b9593f6ca26ec42481c53a6b39f6cddd894ad5c062c1419fac58825`). A busca exata desse SHA na tabela `functions` retornou zero entradas. O README do vendor declara 169.778 funções e um jogo completo, divergente do banco entregue neste snapshot; não inferimos qual título o banco contém.

O CLI `recomp build <rom>` aceita uma imagem genérica; nesta ROM encontrou estaticamente 16.734 blocos e 107.499 instruções e emitiu a biblioteca sem mapa, sem banco e sem profiling `--ram`. O código gerado chama o interpretador para instruções não inlineáveis e misses, portanto a saída é híbrida. O packager permite saída degradada com interpretador; o modo full recomp (`interpreter = false`) exige mapa de funções e falha se `[labels]` estiver ausente. Seu soak de zero fallbacks valida só os caminhos exercitados; a própria documentação mantém essa limitação explícita. Não há mapa de LeafGreen no banco observado nem menção explícita a LeafGreen/FireRed/Pokémon no checkout.

Para esta LeafGreen, o vendor está à frente do projeto local em capacidade de gerar e executar uma tradução: o build passou pela instrução que ainda bloqueia nossa discovery, e a saída concordou com o interpretador por 60 frames. Isso não é prova de jogo completo nem de strict-AOT; a saída executada tem fallback e o primeiro minuto não cobre rotas, saves ou branches computados profundos. Este projeto mantém o contrato diferente: strict-AOT recusa instrução alcançável sem lowering e não cai para interpretação. Recomendo usar o vendor como fonte prioritária de semântica, inventário ISA e arquitetura do runtime, preservando testes e o contrato local estrito. A observação do opcode está em [SRC-47](KNO-001-evidence.md); nenhum código foi transplantado nesta avaliação.

## Evidência por afirmação

| Afirmação | Tipo | Evidência e limite |
|---|---|---|
| O vendor decodifica `0x428A` como `CMP r2,r1` | Fonte | `crates/armv4t/src/thumb.rs:242-292`; Format 4 subopcode A cria `AluOp::Cmp` com os campos de registrador. Os testes próximos cobrem CMP imediato e high-register, mas não encontrei fixture direta para CMP low-register Format 4. O raw da ROM é `8A 42` no offset `0xC00`; a classificação é inferência estática, não trace guest. |
| O CMP não grava Rd e atualiza NZCV | Fonte | `crates/gba-core/src/exec.rs:471-520` define subtração, carry sem borrow, overflow e impede writeback quando `op.is_compare()`. `crates/recomp-core/src/emit.rs:243-268` emite a comparação nativamente e omite a atribuição ao registrador destino. Isso valida a leitura do código de referência, não este recompiler local nem hardware. |
| O vendor emite uma biblioteca para a LeafGreen fixada | Execução | `recomp build '<ROM>'` concluiu com exit 0 e criou `out/Pokemon - Leaf Green Version (U) (V1.1).dylib`, Mach-O arm64, 6.101.744 bytes, SHA-256 `c0fa9322f2d1f5146a5e515b1730ecef40612d2b8bfea97544bb628e1fa85991`. Após o teste, a biblioteca foi mantida em `target/vendor-leafgreen/Pokemon - Leaf Green Version (U) (V1.1).dylib`. O log e os parâmetros estão em [evidência estruturada](evidence/vendor-leafgreen-20261014.json). O comando não recebeu mapa, gamedb ou profiling RAM. |
| A biblioteca executa de modo diferencialmente equivalente no início | Execução | `runc --frames 60` e `verify --frames 60 --reuse` produziram o mesmo hash `7af84b7e8461e452`. No `runc`, foram contados 4.090 blocos nativos e 35.586 misses de despacho. O contador não inclui necessariamente cada chamada aos shells `interp_one`; é uma amostra de 60 frames, não cobertura do jogo. Logs e hashes estão no JSON acima. |
| O mapa embutido não cobre a LeafGreen fixada | Observação local | SQLite foi aberto somente em modo read-only. `gamedb_meta` registra 1.281 funções; a tabela contém um SHA, `2dac7cdf9821c7ef672b5700f0114cf8bbe46bd019f85b97478e4c9a937637c5`; consulta exata pelo SHA da LeafGreen retornou 0. Banco SHA-256 `459078aa1a3f22a316211c5d983794c291a6faea841cd37cbabd8f9fb0461ab6`. |
| Saída full recomp exige mapa e o soak tem cobertura limitada | Declaração + fonte | `crates/pack/src/config.rs:79-108`, `crates/pack/src/main.rs:238-255`, `docs/packaging.md:116-140`. Full recomp sem mapa falha; o soak não prova caminhos não exercitados. |
| O README declara números diferentes do banco observado | Comparação local | `README.md:13-17` declara 169.778 funções e um jogo mapeado; os metadados do arquivo anexado indicam 1.281. É divergência entre documentação e artefato desse checkout, não conclusão sobre outro snapshot ou contagem externa. |

## Decisão de uso como referência

Consultar o vendor para novos opcodes e runtime é útil, mantendo o commit e os caminhos de fonte neste registro. Para `CMP` por registrador, a próxima spec pode usar a semântica observada em SRC-47 e exigir testes/IR/lowering locais. Não copiar arquivos ou trechos nesta avaliação; o vendor declara `MIT OR Apache-2.0`, e qualquer reutilização futura precisa registrar proveniência e preservar os avisos aplicáveis. Não alterar `vendor-ref/` para implementar o produto.

## Questão em aberto

O vendor consegue produzir uma biblioteca híbrida e executá-la com resultado igual ao próprio interpretador por 60 frames nesta LeafGreen; isso foi verificado. Continua em aberto se o vendor consegue produzir um pacote full recomp para a ROM: falta um mapa de funções correspondente e não foi feito soak longo cobrindo caminhos representativos. A execução completa do título e a equivalência com hardware também seguem sem prova.
