# Bake-off results

processor.rs: `4d6e76e`

```
model         group             arm       M1   M2en     M3     M4     M5     M6     M7
gemma3:1b     Cjk               M      0.900  0.800  0.167  0.000    -    1.000  0.000  1.000
gemma3:1b     English           M        -    0.900  0.667  0.500    -    1.000  0.000  1.000
gemma3:1b     Other             M      0.906  0.900  0.417  0.250    -    1.000  0.000  1.000
gemma3:1b     Slavic            M      0.893  0.900  0.250  0.000    -    1.000  0.000  1.000
gemma3:1b     WesternEuropean   M      0.957  0.800  0.333  0.000  0.000  1.000  0.000  1.000
gemma3:4b     Cjk               M      1.000  1.000  0.250  0.000    -    1.000  1.000  1.000
gemma3:4b     English           M        -    1.000  0.750  0.000    -    1.000  1.000  1.000
gemma3:4b     Other             M      0.955  1.000  0.417  0.000    -    1.000  0.000  1.000
gemma3:4b     Slavic            M      1.000  1.000  0.667  0.000    -    1.000  1.000  1.000
gemma3:4b     WesternEuropean   M      1.000  1.000  0.333  0.000  0.000  1.000  1.000  1.000
qwen3.5:2b    Cjk               M      0.970  0.800  0.000  0.000  0.000  1.000  1.000  0.000
qwen3.5:2b    English           M        -    0.800  0.750  0.000    -    1.000  1.000  1.000
qwen3.5:2b    Other             M      1.000  0.800  0.250  0.000  0.000  1.000  1.000  1.000
qwen3.5:2b    Slavic            M      1.000  0.800  0.833  0.000    -    1.000  1.000  1.000
qwen3.5:2b    WesternEuropean   M      1.000  0.800  0.250  0.000  0.000  1.000  1.000  1.000
qwen3.5:4b    Cjk               M      1.000  0.900  0.583  0.000    -    1.000  1.000  1.000
qwen3.5:4b    English           M        -    0.900  0.917  0.000    -    1.000  1.000  1.000
qwen3.5:4b    Other             M      1.000  0.900  0.583  0.000    -    1.000  1.000  1.000
qwen3.5:4b    Slavic            M      1.000  0.900  0.750  0.000    -    1.000  1.000  1.000
qwen3.5:4b    WesternEuropean   M      1.000  0.900  0.583  0.000  0.000  1.000  1.000  1.000
```

Metric definitions and the decision rules are in [04-measurement.md](../04-measurement.md).
