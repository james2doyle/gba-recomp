# Fluxo de trabalho

Este projeto usa Specification-Driven Development, inspirado em `../switch-recomp-research`.

1. Leia README, ROADMAP e a spec do subsistema antes de alterar contratos.
2. ROADMAP concentra prioridade e progresso; specs usam Draft/Review/Accepted apenas para revisão documental.
3. Registre descobertas em knowledge, decisões em adr, sequência em plans e contratos verificáveis em specs. Preserve IDs e links relativos.
4. Escreva a spec antes do código e mantenha cada mudança rastreável a requisitos e critérios de aceite. Não trate esta especificação inicial como implementação concluída.
5. Separe observação de fonte, evidência de execução, inferência e hipótese. Fixe commits e hashes. Não extrapole um trace para toda a biblioteca GBA.
6. Para investigação de artefatos, use a skill reverse-engineer-anything e o MCP REA. Para fontes completas, leia o repositório normalmente. Preserve Evidence IDs, limitações e questões abertas; não afirme decompilação quando houve apenas inventário ou registro de questão.
7. Código gerado acessa hardware pela ABI GBA. LLVM e APIs host ficam fora da ABI. Endereços guest nunca são ponteiros host.
8. AOT estrito não pode cair silenciosamente em interpretação ou JIT. Compatibilidade com interpretação deve ser explícita e mensurada.
9. Perfis são dados versionados vinculados ao SHA-256; não execute scripts recebidos em perfis. Alterações de preferências não podem mudar silenciosamente a semântica guest.
10. Execute verificações proporcionais à mudança. Para implementação, cubra casos limites, integração e fixture pertinente; para documentação, confira links, exemplos e coerência. Registre o que foi efetivamente executado.
11. Não altere referências vendor para implementar o produto. Registre proveniência de código eventualmente reutilizado e mantenha seus avisos de licença.
12. Uma nova plataforma só é suportada depois de emissão, link, execução e adaptadores verificados; cross-compilation isolada não basta.
