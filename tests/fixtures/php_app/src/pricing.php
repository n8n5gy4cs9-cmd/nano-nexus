<?php
class cart_item {
    public $name;
    public $price;

    public function __construct($name, $price) {
        $this->name = $name;
        $this->price = $price;
    }
}

function calcTotal($items) {
    $tmp = 0;
    foreach ($items as $item) {
        $tmp += $item->price;
    }
    return $tmp;
}
