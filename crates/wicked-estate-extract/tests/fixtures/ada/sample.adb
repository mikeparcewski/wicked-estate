with Ada.Text_IO;         use Ada.Text_IO;
with Ada.Float_Text_IO;   use Ada.Float_Text_IO;

package body Sample is

   function Factorial (N : Natural) return Natural is
   begin
      if N = 0 then
         return 1;
      else
         return N * Factorial (N - 1);
      end if;
   end Factorial;

   function Is_Prime (N : Positive) return Boolean is
   begin
      if N < 2 then
         return False;
      end if;
      if N = 2 then
         return True;
      end if;
      if N mod 2 = 0 then
         return False;
      end if;
      declare
         D : Positive := 3;
      begin
         while D * D <= N loop
            if N mod D = 0 then
               return False;
            end if;
            D := D + 2;
         end loop;
      end;
      return True;
   end Is_Prime;

   procedure Print_Primes_Up_To (Limit : Positive) is
   begin
      Put_Line ("Primes up to" & Positive'Image (Limit) & ":");
      for I in 2 .. Limit loop
         if Is_Prime (I) then
            Put (Positive'Image (I));
         end if;
      end loop;
      New_Line;
   end Print_Primes_Up_To;

end Sample;
