"""
QilbeeDB Python SDK setup.
"""

from setuptools import setup, find_packages

with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

setup(
    name="qilbeedb",
    version="0.1.0",
    author="QilbeeDB Team",
    author_email="support@qilbeedb.com",
    description="Python SDK for QilbeeDB - Enterprise Graph Database with Agent Memory",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/your-org/qilbeedb",
    project_urls={
        "Bug Tracker": "https://github.com/your-org/qilbeedb/issues",
        "Documentation": "https://docs.qilbeedb.com",
        "Source Code": "https://github.com/your-org/qilbeedb",
    },
    packages=find_packages(),
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "Topic :: Database",
        "Topic :: Software Development :: Libraries :: Python Modules",
        "License :: OSI Approved :: Apache Software License",
        "Programming Language :: Python :: 3",
        "Programming Language :: Python :: 3.8",
        "Programming Language :: Python :: 3.9",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Programming Language :: Python :: 3.12",
    ],
    python_requires=">=3.8",
    install_requires=[
        "requests>=2.28.0",
    ],
    extras_require={
        "dev": [
            "pytest>=7.0.0",
            "pytest-cov>=4.0.0",
            "black>=23.0.0",
            "flake8>=6.0.0",
            "mypy>=1.0.0",
        ],
    },
    keywords="graph database nosql agent memory ai temporal",
)
