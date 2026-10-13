# SPEC-001 — Intake, identidade e manifesto

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema, escopo e termos

Transformar uma ROM raw `.gba` em imagem imutável identificada. ELF ARM opcional fornece símbolos e seções somente quando sua correspondência com a ROM for demonstrada; containers comprimidos e multiboot ficam para extensão própria. Header descreve metadados, não é prova de autenticidade.

## Requisitos normativos

- IMG-01: intake MUST calcular SHA-256 sobre todos os bytes originais e preservar tamanho e origem. Nenhuma normalização/padding pode alterar a identidade silenciosamente.
- IMG-02: parser MUST validar limites antes de ler o header de 0xC0 bytes; extrair title, game code, maker, revisão, byte fixo e checksum complementar. Header inválido MUST gerar diagnóstico; override explícito para homebrew deve ficar no manifesto.
- IMG-03: baseline MUST aceitar somente imagens entre 0xC0 e 32 MiB. ROMs especiais maiores exigem extensão de mapper; tamanho não potência de dois MUST ter mapping explicitamente definido antes da execução.
- IMG-04: descoberta MUST iniciar no endereço guest `0x08000000` em ARM, interpretando a instrução de entrada sem assumir um offset fixo após o header. Boot HLE e reset BIOS são caminhos distintos.
- IMG-05: manifestos MUST incluir hash ROM, digest perfil compilado, versões IR/ABI/compiler/LLVM, target triple/CPU/features, modo de execução, política BIOS, lista de objetos/assets e hashes. Caminho absoluto local não participa da identidade do build.
- IMG-06: ELF/símbolos e hints MUST ser vinculados a regiões e hashes verificáveis; discrepância é erro, não preferência arbitrária de bytes. Game code serve para busca, nunca seleciona perfil automaticamente sem hash.

## Interfaces, saídas e erros

`analyze(path, profile?) → ImageManifest + CoverageReport`. Relatório JSON versionado com erros `image_too_small`, `image_too_large`, `header_invalid`, `image_hash_mismatch`, `symbols_mismatch`, campos offset/endereço e ação sugerida. CLI distingue erro de entrada de erro interno e não publica manifesto de sucesso parcial.

## Observabilidade e limites

O relatório mostra bytes classificados como código, dados e desconhecidos; soma consistente com regiões sobrepostas explicitadas. Não exporta bytes da ROM por padrão. Header válido não significa software suportado.

## Critérios de aceite e matriz

| Caso | Resultado esperado |
|---|---|
| Imagem truncada em cada campo | Erro com limite, sem acesso fora do buffer |
| Mesma ROM em caminhos distintos | Mesma identidade |
| Mudança de um byte/header/patch | Hash diferente e perfil rejeitado |
| Checksum incorreto | Erro; override explícito auditável |
| Símbolos incompatíveis | Rejeição sem alterar imagem |

## Questões abertas e referências

Mapeadores especiais e seleção de fixtures em KNO-003. Fontes: KNO-001 SRC-04/08; `vendor-ref/mgba/include/mgba/internal/gba/`.
