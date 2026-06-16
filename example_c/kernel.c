#include <stdio.h>
#include <stdlib.h>

typedef struct node {
    int data;
    struct node* next;
} node_t;

node_t* create_node(int val) {
    node_t* n = malloc(sizeof(node_t));
    n->data = val;
    n->next = NULL;
    return n;
}

void print_node(node_t* n) {
    if (n) printf("Node: %d\n", n->data);
}

int compute_sum(node_t* head) {
    int sum = 0;
    while (head) {
        sum += head->data;
        head = head->next;
    }
    return sum;
}

int main() {
    node_t* head = create_node(1);
    head->next = create_node(2);
    head->next->next = create_node(3);
    print_node(head);
    int total = compute_sum(head);
    printf("Sum: %d\n", total);
    // free memory
    while (head) {
        node_t* temp = head;
        head = head->next;
        free(temp);
    }
    return 0;
}
