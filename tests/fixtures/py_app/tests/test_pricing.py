from util.pricing import calc_total, cart_item


def test_total():
    assert calc_total([cart_item("a", 1)]) == 1
