local pricing = require("util.pricing")

local items = { { name = "pen", price = 2.5 } }
print(pricing.calc_total(items))
