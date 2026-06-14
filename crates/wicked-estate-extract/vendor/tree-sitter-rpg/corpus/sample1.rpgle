**free
ctl-opt dftactgrp(*no) actgrp('CIQ');

dcl-f custfile disk usage(*input);

dcl-s totalSales packed(11:2);
dcl-c MAX_ROWS 100;

dcl-ds customer qualified;
  id packed(7:0);
  custname char(50);
end-ds;

// compute a line total
dcl-proc calcTotal export;
  dcl-pi *n packed(11:2);
    qty packed(5:0) const;
    price packed(7:2) const;
  end-pi;
  return qty * price;
end-proc;

dcl-proc main;
  dcl-pi *n;
  end-pi;
  dcl-s i int(10);
  totalSales = calcTotal(10 : 2.50);
  doSomething();
  for i = 1 to MAX_ROWS;
    if i > 50 and totalSales > 0;
      logmsg('big');
    elseif i = 0;
      logmsg('zero');
    else;
      callp logIt('small');
    endif;
  endfor;
  return;
end-proc;
