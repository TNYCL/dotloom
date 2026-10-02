# Renderer assets

`Inter-Regular-subset.ttf` is a subset of **Inter 4.1 Regular**
(<https://github.com/rsms/inter>, release `v4.1`, file `extras/ttf/Inter-Regular.ttf`),
licensed under the SIL Open Font License 1.1 (see `OFL.txt`). Inter declares no
Reserved Font Name.

It was produced with fontTools 4.66.1:

```sh
python -m fontTools.subset extras/ttf/Inter-Regular.ttf \
  --unicodes="U+0020-007E,U+00A0-024F,U+0370-03FF,U+0400-045F,U+2010-205E,U+20A0-20C0,U+2100-214F,U+2190-21FF,U+2200-22FF,U+2300,U+2318,U+25A0-25FF,U+FFFD" \
  --layout-features='kern' --no-hinting --desubroutinize --name-IDs='*' \
  --output-file=Inter-Regular-subset.ttf
```

Coverage: Basic Latin, Latin-1, Latin Extended-A/B (including Turkish
ç ğ ı İ ö ş ü), Greek, basic Cyrillic, general punctuation, currency, letterlike
symbols, arrows, mathematical operators and geometric shapes. Inter has no
U+2300 DIAMETER SIGN; the renderer substitutes U+2205 EMPTY SET.
