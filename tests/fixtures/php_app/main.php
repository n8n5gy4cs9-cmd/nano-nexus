<?php
require_once 'src/pricing.php';

$items = [new cart_item('pen', 2.5)];
echo calcTotal($items);
