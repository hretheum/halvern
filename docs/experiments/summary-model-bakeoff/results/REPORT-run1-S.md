# Bake-off results

processor.rs: `4d6e76e`

```
model         group             arm       M1   M2en     M3     M4     M5     M6     M7
gemma3:1b     Cjk               S      0.900  0.800  0.167  0.250    -    1.000  0.000  1.000
gemma3:1b     English           S        -    0.900  0.750  0.250    -    1.000  0.000  1.000
gemma3:1b     Other             S      0.962  0.800  0.333  0.000  0.000  1.000  0.000  1.000
gemma3:1b     Slavic            S      1.000  0.900  0.333  0.000    -    1.000  0.000  1.000
gemma3:1b     WesternEuropean   S      1.000  0.900  0.083  0.000  0.000  1.000  0.000  1.000
gemma3:4b     Cjk               S      1.000  1.000  0.250  0.250    -    1.000  1.000  1.000
gemma3:4b     English           S        -    1.000  0.833  0.000    -    1.000  1.000  1.000
gemma3:4b     Other             S      0.969  1.000  0.250  0.000    -    1.000  0.000  1.000
gemma3:4b     Slavic            S      1.000  1.000  0.583  0.000    -    1.000  1.000  1.000
gemma3:4b     WesternEuropean   S      1.000  1.000  0.500  0.000  0.000  1.000  1.000  1.000
qwen3.5:2b    Cjk               S      0.960  0.800  0.500  0.000    -    1.000  1.000  1.000
qwen3.5:2b    English           S        -    0.800  0.750  0.000    -    1.000  1.000  1.000
qwen3.5:2b    Other             S      1.000  0.800  0.250  0.000  0.000  1.000  1.000  1.000
qwen3.5:2b    Slavic            S      1.000  0.800  0.667  0.000    -    1.000  1.000  1.000
qwen3.5:2b    WesternEuropean   S      1.000  0.800  0.250  0.000  0.000  1.000  1.000  1.000
qwen3.5:4b    Cjk               S      1.000  0.900  0.583  0.000    -    1.000  1.000  1.000
qwen3.5:4b    English           S        -    0.900  1.000  0.000    -    1.000  1.000  1.000
qwen3.5:4b    Other             S      1.000  0.900  0.417  0.000    -    1.000  1.000  1.000
qwen3.5:4b    Slavic            S      1.000  0.900  0.750  0.000    -    1.000  1.000  1.000
qwen3.5:4b    WesternEuropean   S      1.000  0.900  0.583  0.000  0.000  1.000  1.000  1.000
```

Metric definitions and the decision rules are in [04-measurement.md](../04-measurement.md).
