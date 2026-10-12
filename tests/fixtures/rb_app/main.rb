require_relative 'lib/pricing'

items = [CartItem.new('pen', 2.5)]
puts calc_total(items)
