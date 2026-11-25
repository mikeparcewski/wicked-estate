package cov;

public class Dog extends Pet implements Animal {
  private Pet friend;

  @Override
  public String sound() {
    return "woof";
  }

  public Pet friend() {
    return friend;
  }

  public static Dog make() {
    return new Dog();
  }
}
