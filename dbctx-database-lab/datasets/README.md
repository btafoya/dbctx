# dbctx Database Lab Datasets

This directory contains deterministic CSV seed datasets for every supported
database engine.  Each size contains the same logical entities with different
row counts.

## Sizes

| Size  | Users | Companies | Products | Orders | Order Items |
| ----- | ----- | --------- | -------- | ------ | ----------- |
| small | 10    | 5         | 20       | 50     | 150         |
| medium| 100   | 20        | 100      | 500    | 1,500       |
| large | 1,000 | 50        | 500      | 5,000  | 15,000      |

## Files

Each subdirectory contains:

- `users.csv`
- `companies.csv`
- `products.csv`
- `orders.csv`
- `order_items.csv`

## Regenerating

```bash
bash datasets/generate.sh [small|medium|large]
```

Run without an argument to regenerate all three sizes.

## Loading into a database

Engine-specific loaders can be added under `scripts/load-*` if needed.  The CSV
headers match the canonical schema so generic loaders can map columns directly.
