TAX_RATE = 0.2


class cart_item:
    def __init__(self, name, price):
        self.name = name
        self.price = price

    def doStuff2(self, p):
        tmp = self.price * p
        return tmp


def calc_total(items):
    total = 0
    for it in items:
        total += it.price
    return total
