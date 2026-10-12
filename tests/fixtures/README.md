# Fixture projects (M09)

Six tiny hand-written "shop" apps used by Rename Forge tests. Each has the same story:
`calc_total` (named per language convention) is defined in a pricing module and used from the entry
point, plus deliberately bad names (`cart_item`, `doStuff2`, `tmp`, `Helper_Funcs`), a string lookup of
the function name, imports, a config file naming files/entries, and nested folders.
Do not "fix" them: the tests rely on the flaws. Tests must copy a fixture to a temp dir before changing it.

P1 languages (T09.18): `kt_app`, `swift_app`, `php_app`, `rb_app`, `c_app`, `cpp_app`, `lua_app` follow the same story in smaller form (`calc_total` per convention, `cart_item`, `tmp`, an import/include/require).
