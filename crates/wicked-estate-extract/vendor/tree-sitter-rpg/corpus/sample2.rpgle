**free
ctl-opt main(process);

dcl-proc process;
  dcl-pi *n;
    cmd char(10) const;
  end-pi;
  dcl-s idx int(10);
  dcl-s found ind;

  found = *off;
  idx = %scan('X' : cmd);
  dow idx > 0 and not found;
    select;
      when cmd = 'OPEN';
        openFile();
        found = *on;
      when cmd = 'CLOSE';
        closeFile();
      other;
        logIt(%trim(cmd));
    endsl;
    idx -= 1;
  enddo;

  monitor;
    risky();
  on-error;
    logIt('error');
  endmon;
end-proc;
