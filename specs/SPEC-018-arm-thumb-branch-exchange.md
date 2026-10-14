# SPEC-018 — Branch exchange ARM/Thumb em AOT estrito

Estado do documento: Accepted. Progresso de implementação: [ROADMAP](../ROADMAP.md).

## Escopo

Adicionar `BX Rm` ARM e Thumb, trocar o bit T do CPSR conforme o bit 0 do operando e despachar para código AOT do modo correspondente. A descoberta deve fechar a lista de destinos a partir de valores estáticos comprovados pelo subconjunto implementado. Este contrato não introduz interpretação, JIT ou uma aceitação genérica de destinos dinâmicos.

## Requisitos normativos

- BX-01: reconhecer o encoding ARM `cond 000100101111111111110001 Rm` e Thumb `010001 11 Rm 000`; aceitar `Rm=r0-r15`. BX Thumb é incondicional. BX ARM usa a condição ARM já definida no contrato.
- BX-02: leitura `Rm=PC` usa `guest_pc+8` em ARM e `guest_pc+4` em Thumb, módulo 2^32. Outros registradores são lidos do banco ativo do runtime.
- BX-03: se o bit 0 do alvo for 1, ativar CPSR.T e definir PC como `target & ~1`. Se for 0, limpar CPSR.T e definir PC como `target & ~3`. Não alterar outros campos de CPSR nem aplicar a regra `BX` a outras escritas de PC.
- BX-04: BX ARM com condição falsa preserva registradores/CPSR e avança PC quatro bytes; BX Thumb sempre troca modo e não tem caminho de fallthrough. Cada instrução contabiliza uma unidade de ciclo abstrato.
- BX-05: durante a descoberta, manter valores guest constantes ou conjuntos finitos de constantes apenas quando todos os caminhos analisados os comprovem. Cada valor possível no operando BX deve mapear a uma instrução descoberta e decodificável no modo selecionado. Target desconhecido, conjunto que excede o limite, target fora da ROM, ou target sem código compilável MUST falhar antes de publicar pacote.
- BX-06: o dispatcher AOT seleciona código por par `(modo, PC)`. Um endereço presente somente no outro modo não é correspondência. Destino inesperado em runtime retorna `missing_code`; não há fallback ou chamada a interpretador/JIT.
- BX-07: usar CPSR.T e PC já existentes no ABI v3; não reinterpretar o endereço guest como ponteiro host nem alterar layouts v1–v3.
- BX-08: a fixture deve alternar ARM→Thumb→ARM usando duas palavras literais imutáveis, executar os dois BX em O0/O2 e verificar PC, CPSR e ausência de pacote para destino fora do ROM.

## Limites desta tranche

Esta spec define apenas a semântica de `BX`. Na tranche original, o decoder Thumb aceitava somente BX; formatos Thumb adicionais exigem seus próprios contratos, como SPEC-019–SPEC-032. Valores carregados de RAM/MMIO ou calculados por operações não rastreadas são desconhecidos e bloqueiam a compilação do destino indireto. O limite deliberado evita tratar um trace ou um literal como prova de todos os possíveis destinos do jogo.

## Evidência e referências

LeafGreen parou em `0x08000230`, encoding `0xE12FFF11`; a leitura como `BX r1` é inferência pelos campos. SRC-19 registra decoders e semântica nas duas referências fixadas. A fixture proposta prova somente o fluxo sintético ARM/Thumb; não demonstra execução de LeafGreen.
