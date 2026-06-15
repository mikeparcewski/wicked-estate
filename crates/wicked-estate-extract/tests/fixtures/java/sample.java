package com.example.service;

import java.util.List;
import java.util.Optional;
import java.util.stream.Collectors;

public class UserService {
    private final UserRepository repository;

    public UserService(UserRepository repository) {
        this.repository = repository;
    }

    public Optional<User> findById(long id) {
        return repository.findById(id);
    }

    public List<User> findActive() {
        return repository.findAll().stream()
            .filter(User::isActive)
            .collect(Collectors.toList());
    }

    public User create(String name, String email) {
        User user = new User(name, email);
        return repository.save(user);
    }

    public void deactivate(long id) {
        repository.findById(id).ifPresent(u -> {
            u.setActive(false);
            repository.save(u);
        });
    }
}
