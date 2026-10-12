class CartItem
  attr_reader :name, :price

  def initialize(name, price)
    @name = name
    @price = price
  end
end

def calc_total(items)
  tmp = 0
  items.each { |item| tmp += item.price }
  tmp
end
