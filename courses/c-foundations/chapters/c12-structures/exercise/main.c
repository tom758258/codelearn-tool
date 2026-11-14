#include <stdio.h>

typedef enum {
    AVAILABLE,
    SOLD_OUT
} StockState;

typedef struct {
    const char *name;
    int quantity;
    int unit_cents;
    StockState state;
} Product;

int main(void)
{
    const Product product = {"Pencil", 3, 15, AVAILABLE};
    if (product.state == AVAILABLE) {
        printf("%s x%d: %d cents\n",
               product.name, product.quantity,
               product.quantity * product.unit_cents);
    }
    return 0;
}
