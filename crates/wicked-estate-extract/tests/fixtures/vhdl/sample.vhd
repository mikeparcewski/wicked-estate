library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

-- 8-bit up/down counter with synchronous reset and load.
entity counter_8 is
    port (
        clk     : in  std_logic;
        rst     : in  std_logic;             -- synchronous reset
        en      : in  std_logic;             -- count enable
        up_down : in  std_logic;             -- '1' = up, '0' = down
        load    : in  std_logic;             -- parallel load strobe
        d_in    : in  std_logic_vector(7 downto 0);
        q       : out std_logic_vector(7 downto 0);
        tc      : out std_logic              -- terminal count
    );
end entity counter_8;

architecture rtl of counter_8 is
    signal count : unsigned(7 downto 0) := (others => '0');
begin

    -- Count process
    count_proc : process(clk)
    begin
        if rising_edge(clk) then
            if rst = '1' then
                count <= (others => '0');
            elsif load = '1' then
                count <= unsigned(d_in);
            elsif en = '1' then
                if up_down = '1' then
                    count <= count + 1;
                else
                    count <= count - 1;
                end if;
            end if;
        end if;
    end process count_proc;

    -- Output assignments
    q  <= std_logic_vector(count);
    tc <= '1' when (up_down = '1' and count = x"FF") or
                   (up_down = '0' and count = x"00")
               else '0';

end architecture rtl;


-- Simple 2-to-1 mux used as a sub-component in the testbench wrapper.
entity mux2to1_8 is
    port (
        sel : in  std_logic;
        a   : in  std_logic_vector(7 downto 0);
        b   : in  std_logic_vector(7 downto 0);
        y   : out std_logic_vector(7 downto 0)
    );
end entity mux2to1_8;

architecture rtl of mux2to1_8 is
begin
    sel_proc : process(sel, a, b)
    begin
        if sel = '0' then
            y <= a;
        else
            y <= b;
        end if;
    end process sel_proc;
end architecture rtl;


-- Wrapper that ties the counter and mux together.
entity counter_with_mux is
    port (
        clk     : in  std_logic;
        rst     : in  std_logic;
        en      : in  std_logic;
        up_down : in  std_logic;
        load    : in  std_logic;
        preset  : in  std_logic_vector(7 downto 0);
        alt     : in  std_logic_vector(7 downto 0);
        sel     : in  std_logic;
        q       : out std_logic_vector(7 downto 0);
        tc      : out std_logic
    );
end entity counter_with_mux;

architecture structural of counter_with_mux is
    signal mux_out : std_logic_vector(7 downto 0);

    component counter_8 is
        port (
            clk     : in  std_logic;
            rst     : in  std_logic;
            en      : in  std_logic;
            up_down : in  std_logic;
            load    : in  std_logic;
            d_in    : in  std_logic_vector(7 downto 0);
            q       : out std_logic_vector(7 downto 0);
            tc      : out std_logic
        );
    end component;

    component mux2to1_8 is
        port (
            sel : in  std_logic;
            a   : in  std_logic_vector(7 downto 0);
            b   : in  std_logic_vector(7 downto 0);
            y   : out std_logic_vector(7 downto 0)
        );
    end component;

begin

    u_mux : mux2to1_8
        port map (sel => sel, a => preset, b => alt, y => mux_out);

    u_cnt : counter_8
        port map (
            clk     => clk,
            rst     => rst,
            en      => en,
            up_down => up_down,
            load    => load,
            d_in    => mux_out,
            q       => q,
            tc      => tc
        );

end architecture structural;
