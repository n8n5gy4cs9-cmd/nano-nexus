# Fixture projects (M09)

Six tiny hand-written "shop" apps used by Rename Forge tests. Each has the same story:
`calc_total` (named per language convention) is defined in a pricing module and used from the entry
point, plus deliberately bad names (`cart_item`, `doStuff2`, `tmp`, `Helper_Funcs`), a string lookup of
the function name, imports, a config file naming files/entries, and nested folders.
Do not "fix" them: the tests rely on the flaws. Tests must copy a fixture to a temp dir before changing it.
