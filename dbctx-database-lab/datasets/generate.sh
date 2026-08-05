#!/usr/bin/env bash
# Generate deterministic CSV seed datasets for the dbctx database lab.
# Usage: generate.sh [small|medium|large]
# If no size is provided, all three are generated.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    echo "Usage: $0 [small|medium|large]"
    exit 1
}

SIZE="${1:-all}"
case "${SIZE}" in
    small|medium|large|all) ;;
    *) usage ;;
esac

sizes=(small medium large)
if [[ "${SIZE}" != "all" ]]; then
    sizes=("${SIZE}")
fi

generate_users() {
    local count="$1"
    printf "id,username,email,created_at\n"
    for i in $(seq 1 "${count}"); do
        printf "%d,user_%d,user_%d@example.com,2024-01-01 00:00:00\n" "${i}" "${i}" "${i}"
    done
}

generate_companies() {
    local count="$1"
    printf "id,name,created_at\n"
    for i in $(seq 1 "${count}"); do
        printf "%d,Company %d,2024-01-01 00:00:00\n" "${i}" "${i}"
    done
}

generate_products() {
    local count="$1"
    local company_count="$2"
    printf "id,company_id,name,price,created_at\n"
    for i in $(seq 1 "${count}"); do
        local company_id=$(( ((i - 1) % company_count) + 1 ))
        local price
        price=$(awk "BEGIN { printf \"%.2f\", 9.99 + (($i - 1) % 90) * 1.00 }")
        printf "%d,%d,Product %d,%s,2024-01-01 00:00:00\n" "${i}" "${company_id}" "${i}" "${price}"
    done
}

generate_orders() {
    local count="$1"
    local user_count="$2"
    printf "id,user_id,total,created_at\n"
    for i in $(seq 1 "${count}"); do
        local user_id=$(( ((i - 1) % user_count) + 1 ))
        printf "%d,%d,0.00,2024-01-01 00:00:00\n" "${i}" "${user_id}"
    done
}

generate_order_items() {
    local count="$1"
    local order_count="$2"
    local product_count="$3"
    printf "id,order_id,product_id,quantity,price\n"
    for i in $(seq 1 "${count}"); do
        local order_id=$(( ((i - 1) % order_count) + 1 ))
        local product_id=$(( ((i - 1) % product_count) + 1 ))
        local quantity=$(( (i % 5) + 1 ))
        local price
        price=$(awk "BEGIN { printf \"%.2f\", 9.99 + ((($i - 1) % 90)) * 1.00 }")
        printf "%d,%d,%d,%d,%s\n" "${i}" "${order_id}" "${product_id}" "${quantity}" "${price}"
    done
}

generate_size() {
    local size="$1"
    local users companies products orders items
    case "${size}" in
        small)
            users=10; companies=5; products=20; orders=50; items=150 ;;
        medium)
            users=100; companies=20; products=100; orders=500; items=1500 ;;
        large)
            users=1000; companies=50; products=500; orders=5000; items=15000 ;;
    esac

    local dir="${SCRIPT_DIR}/${size}"
    mkdir -p "${dir}"

    echo "Generating ${size} dataset..."
    generate_users "${users}"     > "${dir}/users.csv"
    generate_companies "${companies}" > "${dir}/companies.csv"
    generate_products "${products}" "${companies}" > "${dir}/products.csv"
    generate_orders "${orders}" "${users}" > "${dir}/orders.csv"
    generate_order_items "${items}" "${orders}" "${products}" > "${dir}/order_items.csv"

    echo "  users:     ${users}"
    echo "  companies: ${companies}"
    echo "  products:  ${products}"
    echo "  orders:    ${orders}"
    echo "  items:     ${items}"
}

main() {
    if ! command -v awk >/dev/null 2>&1; then
        echo "ERROR: awk is required." >&2
        exit 1
    fi

    for size in "${sizes[@]}"; do
        generate_size "${size}"
    done

    echo "Dataset generation complete."
}

main "$@"
