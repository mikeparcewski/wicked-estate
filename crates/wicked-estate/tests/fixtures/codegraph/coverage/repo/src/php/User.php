<?php
namespace Cov;

interface HasName
{
    public function name(): string;
}

abstract class Model
{
    public function save(): bool { return true; }
}

class User extends Model implements HasName
{
    public function name(): string { return "u"; }

    public function save(): bool
    {
        return parent::save();
    }

    public static function make(): User
    {
        $u = new User();
        $u->save();
        return $u;
    }
}
