// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

interface IToken {
    function transfer(address to, uint256 amount) external returns (bool);
    function balanceOf(address account) external view returns (uint256);
}

contract SimpleToken is IToken {
    string public name;
    uint256 public totalSupply;
    mapping(address => uint256) private _balances;

    event Transfer(address indexed from, address indexed to, uint256 amount);

    modifier onlyPositive(uint256 amount) {
        require(amount > 0, "Amount must be positive");
        _;
    }

    constructor(string memory _name, uint256 _supply) {
        name = _name;
        totalSupply = _supply;
        _balances[msg.sender] = _supply;
    }

    function transfer(address to, uint256 amount) external onlyPositive(amount) returns (bool) {
        _balances[msg.sender] -= amount;
        _balances[to] += amount;
        emit Transfer(msg.sender, to, amount);
        return true;
    }

    function balanceOf(address account) external view returns (uint256) {
        return _balances[account];
    }

    function mint(address to, uint256 amount) internal onlyPositive(amount) {
        _balances[to] += amount;
        totalSupply += amount;
    }
}
