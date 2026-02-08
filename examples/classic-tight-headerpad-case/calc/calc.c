#include <stdio.h>

// no inline
__attribute__((noinline)) int calculate(int a, int b) {
  int result = a + b;
  return result;
}

int main(void) {
  int result = calculate(3, 66);
  printf("Result: %d\n", result);
  return 0;
}
