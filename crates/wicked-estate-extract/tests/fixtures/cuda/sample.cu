#include <stdio.h>
#include <cuda_runtime.h>

#define BLOCK_SIZE 256

// Device helper: compute the square of a value.
__device__ float square(float x) {
    return x * x;
}

// Device helper: clamp a value to [lo, hi].
__device__ float clamp(float x, float lo, float hi) {
    return fmaxf(lo, fminf(hi, x));
}

// Kernel: element-wise apply ReLU then square, writing to out.
__global__ void relu_square_kernel(const float *in, float *out, int n) {
    int idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < n) {
        float v = clamp(in[idx], 0.0f, 1e9f); // ReLU
        out[idx] = square(v);
    }
}

static void check(cudaError_t err, const char *op) {
    if (err != cudaSuccess) {
        fprintf(stderr, "CUDA error in %s: %s\n", op, cudaGetErrorString(err));
        exit(1);
    }
}

int main(void) {
    const int N = 1024;
    float h_in[N], h_out[N];
    float *d_in, *d_out;

    // Initialise with values in [-1, 2].
    for (int i = 0; i < N; i++) {
        h_in[i] = (float)(i - N / 2) / (N / 4);
    }

    check(cudaMalloc(&d_in,  N * sizeof(float)), "malloc d_in");
    check(cudaMalloc(&d_out, N * sizeof(float)), "malloc d_out");
    check(cudaMemcpy(d_in, h_in, N * sizeof(float), cudaMemcpyHostToDevice), "H2D");

    int blocks = (N + BLOCK_SIZE - 1) / BLOCK_SIZE;
    relu_square_kernel<<<blocks, BLOCK_SIZE>>>(d_in, d_out, N);
    check(cudaGetLastError(), "kernel launch");
    check(cudaDeviceSynchronize(), "sync");

    check(cudaMemcpy(h_out, d_out, N * sizeof(float), cudaMemcpyDeviceToHost), "D2H");

    // Spot-check: first negative input maps to 0, first positive maps to x^2.
    printf("h_in[0]=%.3f -> %.3f (expect 0.000)\n", h_in[0], h_out[0]);
    printf("h_in[%d]=%.3f -> %.3f (expect %.3f)\n",
           N / 2 + 1, h_in[N / 2 + 1], h_out[N / 2 + 1],
           h_in[N / 2 + 1] * h_in[N / 2 + 1]);

    cudaFree(d_in);
    cudaFree(d_out);
    return 0;
}
