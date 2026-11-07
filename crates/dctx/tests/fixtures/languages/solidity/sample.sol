pragma solidity ^0.8.0;
contract Boxed {
    modifier ready() { _; }
    function copy(uint value) public ready returns (uint) {
        return value;
    }
}
