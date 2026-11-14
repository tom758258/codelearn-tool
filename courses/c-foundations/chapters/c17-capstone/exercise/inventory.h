#ifndef CODELEARN_INVENTORY_H
#define CODELEARN_INVENTORY_H

#include <stddef.h>

typedef struct {
    const char *name;
    int quantity;
    int unit_cents;
} Product;

int total_cents(const Product products[], size_t count);

#endif
