# SPEC-010 — Destinos LLVM e distribuição

Estado do documento: Review. Revisão GBA-005 em 2026-10-13; aceite da implementação depende dos gates listados abaixo. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Transformar emissão de objetos em aplicativo executável com hardware runtime e menu. Destino LLVM sozinho não garante plataforma suportada.

## Requisitos normativos

- PORT-01: adaptador MUST fornecer tela, áudio, input, storage, clock monotônico e lifecycle pause/resume. Runtime headless MUST executar sem GPU/áudio; backend indisponível tem erro/capability explícita.
- PORT-02: build MUST validar backend LLVM, target triple/features, sysroot, linker, ABI runtime e bibliotecas do destino antes de link. Não usar automaticamente libs host no cross-build.
- PORT-03: pacote MUST ter executável, manifesto, perfil efetivo, assets autorizados necessários, licenças e hashes. Código LLVM/compiler não é requisito de execução. Inclusão de ROM/BIOS requer opção explícita local; alternativa é carregar arquivo externo por hash.
- PORT-04: artefatos MUST identificar strict-aot versus aot-compatible, capacidades, limitações conhecidas e cenários verificados. Bibliotecas/runtime do pacote MUST corresponder à ABI registrada.
- PORT-05: matriz de suporte MUST distinguir `planejado`, `emite objeto`, `linka`, `executa headless`, `frontend validado`. Só o último sustenta distribuição com menu completo.
- PORT-06: paths de cache/save/config MUST usar convenções do SO e ser independentes do diretório corrente. Lifecycle mobile salva backup pendente e reconstrói dispositivos após resume sem alterar tempo guest.
- PORT-07: build reproduzível SHOULD normalizar timestamps e paths; MUST ao menos reproduzir GBA IR/perfil/manifesto sem campos voláteis. Bit-identidade de binário depende de toolchain fixada e deve ser medida separadamente.

## Matriz pretendida

O bootstrap fixa os targets nativos e o LLVM por host em [toolchain.lock.toml](../toolchain.lock.toml). GBA-010 deve registrar versão do linker/sysroot efetivos, produzir e linkar a fixture em cada host e inspecionar ELF/Mach-O. Esses resultados não qualificam, por si só, a distribuição ou compatibilidade de SO; os gates desta spec continuam obrigatórios.

| Ordem | Destinos | Gates adicionais |
|---|---|---|
| P0 | Linux x86-64, macOS arm64 | Objeto, link, headless, menu e dispositivos |
| P1 | Windows x86-64, Linux arm64 | ABI/formato de objeto, paths e packaging |
| P2 | Android arm64, iOS arm64 | SDK, assinatura, lifecycle, touch, storage |
| P3 | WebAssembly e outros backends | Ambiente host, threads/áudio, filesystem, deployment |

Todos estão planejados; não há plataforma validada nesta entrega.

## Aceite, erros e limites

Pacote deve iniciar em máquina limpa sem LLVM, criar saves fora do binário, detectar ROM externa errada, tolerar ausência de áudio e rodar cenário determinístico. Cross-object sem execução permanece no estágio correspondente. Erros `sysroot_missing`, `linker_missing`, `target_unsupported`, `host_capability_missing` orientam o próximo passo.

Referências: KNO-001 fonte LLVM cross-compilation, ADR-001 e SPEC-011.
