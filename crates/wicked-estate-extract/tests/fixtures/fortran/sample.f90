module statistics_mod
  implicit none

contains

  ! Computes the arithmetic mean of an array.
  pure function mean(x) result(m)
    real, intent(in) :: x(:)
    real             :: m
    m = sum(x) / real(size(x))
  end function mean

  ! Computes the sample standard deviation.
  pure function std_dev(x) result(s)
    real, intent(in) :: x(:)
    real             :: s
    real             :: m
    integer          :: n
    n = size(x)
    m = mean(x)
    s = sqrt(sum((x - m)**2) / real(n - 1))
  end function std_dev

  ! Normalises an array to zero mean and unit variance (z-score).
  ! Returns a new array; does not modify the input.
  function normalise(x) result(z)
    real, intent(in) :: x(:)
    real             :: z(size(x))
    real             :: m, s
    m = mean(x)
    s = std_dev(x)
    if (s == 0.0) then
      z = 0.0
    else
      z = (x - m) / s
    end if
  end function normalise

end module statistics_mod


program main
  use statistics_mod
  implicit none

  real :: data(6) = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0]
  real :: z(6)
  integer :: i

  write(*,'(A,F8.4)') 'Mean:    ', mean(data)
  write(*,'(A,F8.4)') 'Std dev: ', std_dev(data)

  z = normalise(data)
  write(*,'(A)') 'Normalised values:'
  do i = 1, size(z)
    write(*,'(2X,F8.4)') z(i)
  end do
end program main
