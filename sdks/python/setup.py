"""
QilbeeDB Python SDK setup.
"""

from setuptools import setup, find_packages

with open("README.md", "r", encoding="utf-8") as fh:
    long_description = fh.read()

setup(
    name="qilbeedb",
    version="0.2.0",
    author="AICUBE TECHNOLOGY LLC",
    author_email="contact@aicube.ca",
    description="Python SDK for QilbeeDB - Enterprise Graph Database with Agent Memory",
    long_description=long_description,
    long_description_content_type="text/markdown",
    url="https://github.com/aicubetechnology/qilbeeDB",
    project_urls={
        "Bug Tracker": "https://github.com/aicubetechnology/qilbeeDB/issues",
        "Documentation": "https://docs.qilbeedb.io",
        "Source Code": "https://github.com/aicubetechnology/qilbeeDB",
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
        "PyJWT>=2.8.0",
        "cryptography>=41.0.0",
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
