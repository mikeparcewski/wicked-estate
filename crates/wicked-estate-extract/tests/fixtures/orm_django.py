"""Django ORM fixture — models.Model subclasses with models.XField(...) attributes."""
from django.db import models

MAX_TITLE_LEN = 200
DEFAULT_STATUS = "draft"


class Category(models.Model):
    name = models.CharField(max_length=100)
    slug = models.SlugField(unique=True)
    description = models.TextField(blank=True)

    class Meta:
        db_table = "categories"
        ordering = ["name"]


class Article(models.Model):
    title = models.CharField(max_length=MAX_TITLE_LEN)
    body = models.TextField()
    pub_date = models.DateTimeField(auto_now_add=True)
    updated_at = models.DateTimeField(auto_now=True)
    views = models.IntegerField(default=0)
    published = models.BooleanField(default=False)
    category = models.ForeignKey(
        Category, on_delete=models.SET_NULL, null=True, blank=True
    )
    author = models.ForeignKey("auth.User", on_delete=models.CASCADE)

    class Meta:
        db_table = "articles"
        ordering = ["-pub_date"]


def get_published():
    return Article.objects.filter(published=True)
