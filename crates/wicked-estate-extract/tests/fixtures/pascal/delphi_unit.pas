unit AccountManager;
{ Delphi-style unit: interface/implementation, class with constructor/destructor,
  qualified method implementations, and procedure calls. Exercises the Delphi
  dialect of tree-sitter-pascal (distinct from the Free Pascal function fixture). }

interface

uses
  System.SysUtils, System.Classes;

type
  TAccount = class(TPersistent)
  private
    FOwner: string;
    FBalance: Currency;
    procedure Validate(Amount: Currency);
  public
    constructor Create(const AOwner: string);
    destructor Destroy; override;
    function GetBalance: Currency;
    procedure Deposit(Amount: Currency);
    procedure Withdraw(Amount: Currency);
  end;

implementation

constructor TAccount.Create(const AOwner: string);
begin
  inherited Create;
  FOwner := AOwner;
  FBalance := 0;
end;

destructor TAccount.Destroy;
begin
  inherited Destroy;
end;

procedure TAccount.Validate(Amount: Currency);
begin
  if Amount <= 0 then
    raise Exception.Create('Amount must be positive');
end;

function TAccount.GetBalance: Currency;
begin
  Result := FBalance;
end;

procedure TAccount.Deposit(Amount: Currency);
begin
  Validate(Amount);
  FBalance := FBalance + Amount;
end;

procedure TAccount.Withdraw(Amount: Currency);
begin
  Validate(Amount);
  if Amount > FBalance then
    raise Exception.Create('Insufficient funds');
  FBalance := FBalance - Amount;
end;

end.
