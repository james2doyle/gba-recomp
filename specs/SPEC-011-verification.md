# SPEC-011 — Conformance e validação por jogo

Estado do documento: Draft. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Problema e escopo

Demonstrar equivalência e compatibilidade dentro de cenários reproduzíveis, preservando limites da evidência. Esta spec define testes futuros, não relata testes de um runtime já existente.

## Requisitos normativos

- VAL-01: fixture MUST identificar fonte/origem, hash ROM/BIOS, perfil, versão de runtime/oráculo, input por ciclo/frame, seed, epoch RTC, duração e condição de sucesso. Testes homebrew sintéticos começam antes de corpus comercial.
- VAL-02: comparar MUST distinguir estado CPU, acessos bus/MMIO, eventos/IRQ, framebuffer bruto e áudio canônico. Primeiro ponto de divergência tem checkpoint e histórico limitado para reprodução.
- VAL-03: interpretador que compartilha decoder/semântica não pode ser único oráculo. mGBA fixado por commit e suites independentes SHOULD fornecer segunda referência; divergência entre oráculos é questão aberta, não prova automática de um lado.
- VAL-04: conformance MUST abranger casos das SPEC-001–010, O0/O2, dispatch/fallback e pelo menos as arquiteturas host iniciais antes de prometer portabilidade. Float host não deve contaminar hashing canônico.
- VAL-05: relatório por jogo MUST listar revisão, cenário, frames/ciclos, instruções AOT/interpretadas, misses, guards invalidados, SWIs/capacidades, primeiro erro e hashes. “Boot”, “menu do jogo”, “jogável no cenário” e “concluído no cenário” são resultados distintos.
- VAL-06: sucesso strict-aot MUST ter zero instruções interpretadas e zero compilação em runtime durante a janela. Relatório MUST dizer que caminhos não percorridos continuam sem comprovação.
- VAL-07: desempenho MUST registrar máquina, build/passes, frequência de apresentação, tempo de compilação, RSS pico, bytes de código e distribuição de tempo runtime. Não publicar ganho sem medição comparável.
- VAL-08: REA Evidence IDs e traces MUST ser preservados com limitações; falha de loader não pode ser substituída por alegação de decompilação. Arquivos de evidência não incluem dumps proprietários por padrão.

## Aceite e sequência

| Nível | Gate |
|---|---|
| Parser/decoder/IR | Casos de borda e verificadores |
| CPU | Golden e diferencial independente |
| Bus/scheduler | DMA, IRQ, WAITCNT e eventos simultâneos |
| End-to-end | Homebrew com input, gráfico, tom de áudio e save |
| Produto | Menu, persistência, dispositivos e pacote limpo |
| Jogo | Cenário fixado, cobertura medida e limitações |

`verify` retorna exit code distinto para sucesso, divergência, capability ausente e infraestrutura indisponível. Corpus não disponível é skipped com motivo, nunca passed. Nenhuma tarefa de implementação vira DONE somente por compilar.

## Fontes e questões

KNO-001/002/003, método SDD do Switch. Limiares de áudio, duração e orçamento de desempenho são fixados por cenário antes da execução, não ajustados para esconder divergências.
