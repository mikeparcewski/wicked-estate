MAX_ITER <- 1000L
DEFAULT_TOL <- 1e-6

normalize <- function(x) {
    rng <- range(x)
    if (rng[2] == rng[1]) return(rep(0, length(x)))
    (x - rng[1]) / (rng[2] - rng[1])
}

summarize_vec <- function(x, digits = 3L) {
    list(
        mean   = round(mean(x), digits),
        median = round(median(x), digits),
        sd     = round(sd(x), digits),
        n      = length(x)
    )
}

run_pipeline <- function(data, max_iter = MAX_ITER, tol = DEFAULT_TOL) {
    normed <- normalize(data)
    stats  <- summarize_vec(normed)
    converged <- stats$sd < tol || max_iter == 0L
    list(stats = stats, converged = converged, iterations = max_iter)
}

# S3 class definition
new_dataset <- function(values, label = "unnamed") {
    obj <- list(values = values, label = label)
    class(obj) <- "Dataset"
    obj
}

print.Dataset <- function(x, ...) {
    cat("Dataset:", x$label, "\n")
    s <- summarize_vec(x$values)
    cat("  n =", s$n, "  mean =", s$mean, "  sd =", s$sd, "\n")
}

ds <- new_dataset(c(1.2, 3.4, 2.1, 5.6, 4.0), label = "sample")
result <- run_pipeline(ds$values)
print(ds)
